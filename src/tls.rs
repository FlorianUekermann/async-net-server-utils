use crate::tcp::TcpIncoming;
use crate::{HttpIncoming, TcpOrTlsIncoming, TcpStream};
use futures::prelude::*;
use futures::stream::{FusedStream, FuturesUnordered};
use futures::StreamExt;
use rustls_acme::futures_rustls::rustls::server::{Acceptor, ClientHello};
use rustls_acme::futures_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
use rustls_acme::futures_rustls::{Accept, LazyConfigAcceptor};
use rustls_pemfile::Item;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

pub type TlsStream = rustls_acme::futures_rustls::server::TlsStream<TcpStream>;

pub struct TlsIncoming<F: FnMut(&ClientHello) -> Arc<ServerConfig>> {
    tcp_incoming: Option<TcpIncoming>,
    f: F,
    start_accepts: FuturesUnordered<LazyConfigAcceptor<TcpStream>>,
    accepts: FuturesUnordered<Accept<TcpStream>>,
}

impl<F: FnMut(&ClientHello) -> Arc<ServerConfig>> TlsIncoming<F> {
    pub fn new(tcp_incoming: TcpIncoming, f: F) -> Self {
        let start_accepts = FuturesUnordered::new();
        let accepts = FuturesUnordered::new();
        TlsIncoming {
            tcp_incoming: Some(tcp_incoming),
            f,
            start_accepts,
            accepts,
        }
    }
    pub fn http(self) -> HttpIncoming<TlsStream, Self> {
        HttpIncoming::new(self)
    }
}

impl<F: FnMut(&ClientHello) -> Arc<ServerConfig> + 'static> TlsIncoming<F> {
    pub fn or_tcp(self) -> TcpOrTlsIncoming {
        let mut tcp_or_tls = TcpOrTlsIncoming::new();
        tcp_or_tls.push(self);
        tcp_or_tls
    }
}

impl<F: FnMut(&ClientHello) -> Arc<ServerConfig>> Unpin for TlsIncoming<F> {}

impl<F: FnMut(&ClientHello) -> Arc<ServerConfig>> Stream for TlsIncoming<F> {
    type Item = TlsStream;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.accepts.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(tls_stream))) => return Poll::Ready(Some(tls_stream)),
                Poll::Ready(Some(Err(err))) => log::debug!("tls accept error: {:?}", err),
                Poll::Ready(None) | Poll::Pending => match self.start_accepts.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok(start_handshake))) => {
                        let config = (self.f)(&start_handshake.client_hello());
                        let accept_fut = start_handshake.into_stream(config);
                        self.accepts.push(accept_fut);
                    }
                    Poll::Ready(Some(Err(err))) => log::debug!("tls accept error: {:?}", err),
                    Poll::Ready(None) | Poll::Pending => match &mut self.tcp_incoming {
                        None => match self.is_terminated() {
                            true => return Poll::Ready(None),
                            false => return Poll::Pending,
                        },
                        Some(tcp_incoming) => match tcp_incoming.poll_next_unpin(cx) {
                            Poll::Ready(Some(tcp_stream)) => {
                                let acceptor = Acceptor::default();
                                let acceptor_fut = LazyConfigAcceptor::new(acceptor, tcp_stream);
                                self.start_accepts.push(acceptor_fut);
                            }
                            Poll::Ready(None) => drop(self.tcp_incoming.take()),
                            Poll::Pending => return Poll::Pending,
                        },
                    },
                },
            }
        }
    }
}

impl<F: FnMut(&ClientHello) -> Arc<ServerConfig>> FusedStream for TlsIncoming<F> {
    fn is_terminated(&self) -> bool {
        self.tcp_incoming.is_none()
            && self.accepts.is_terminated()
            && self.start_accepts.is_terminated()
    }
}

pub fn parse_pem(pem: impl AsRef<[u8]>) -> io::Result<(Vec<Certificate>, PrivateKey)> {
    let mut buf = pem.as_ref();
    let pem = rustls_pemfile::read_all(&mut buf)?;

    let (mut cert_chain, mut private_key) = (Vec::new(), None);
    for item in pem.into_iter() {
        match item {
            Item::X509Certificate(b) => cert_chain.push(Certificate(b)),
            Item::RSAKey(v) | Item::PKCS8Key(v) | Item::ECKey(v) => {
                if private_key.is_none() {
                    private_key = Some(PrivateKey(v));
                }
            }
            _ => {}
        }
    }

    let private_key = match private_key {
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing private key",
            ))
        }
        Some(private_key) => private_key,
    };
    if cert_chain.len() == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing certificates",
        ));
    }
    Ok((cert_chain, private_key))
}
