use crate::tcp::TcpIncoming;
use crate::TcpStream;
use futures::prelude::*;
use futures::stream::{FusedStream, FuturesUnordered};
use futures::StreamExt;
use rustls_acme::futures_rustls::rustls::server::{Acceptor, ClientHello};
use rustls_acme::futures_rustls::rustls::ServerConfig;
use rustls_acme::futures_rustls::{Accept, LazyConfigAcceptor};
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
                        None => return Poll::Pending,
                        Some(tcp_incoming) => match tcp_incoming.poll_next_unpin(cx) {
                            Poll::Ready(Some(tcp_stream)) => {
                                let acceptor = Acceptor::new().unwrap();
                                let acceptor_fut = LazyConfigAcceptor::new(acceptor, tcp_stream);
                                self.start_accepts.push(acceptor_fut);
                            }
                            Poll::Ready(None) => {
                                drop(self.tcp_incoming.take());
                                return Poll::Ready(None);
                            }
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
