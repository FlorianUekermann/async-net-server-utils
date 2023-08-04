use crate::h1::HttpIncoming;
use crate::tls::TlsIncoming;
use crate::{AcmeIncoming, TcpOrTlsIncoming};
use async_io::{Async, ReadableOwned};
use futures::prelude::*;
use futures::stream::FusedStream;
use futures::FutureExt;
use rustls_acme::caches::DirCache;
use rustls_acme::futures_rustls::rustls::server::ClientHello;
use rustls_acme::futures_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
use rustls_acme::AcmeConfig;
use std::fmt::Debug;
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

pub type TcpStream = async_net::TcpStream;

pub struct TcpIncoming {
    listener: Arc<Async<std::net::TcpListener>>,
    readable: Pin<Box<ReadableOwned<std::net::TcpListener>>>,
}

impl TcpIncoming {
    pub fn bind(addr: impl Into<SocketAddr>) -> io::Result<Self> {
        let listener = Arc::new(Async::<std::net::TcpListener>::bind(addr)?);
        let readable = Box::pin(listener.clone().readable_owned());
        Ok(Self { listener, readable })
    }
    pub fn tls_with_config<F: FnMut(&ClientHello) -> Arc<ServerConfig>>(
        self,
        f: F,
    ) -> TlsIncoming<F> {
        TlsIncoming::new(self, f)
    }
    pub fn tls(
        self,
        cert_chain: Vec<Certificate>,
        key_der: PrivateKey,
    ) -> Result<
        TlsIncoming<impl FnMut(&ClientHello) -> Arc<ServerConfig>>,
        rustls_acme::futures_rustls::rustls::Error,
    > {
        let config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key_der)?;
        let config = Arc::new(config);
        Ok(TlsIncoming::new(self, move |_| config.clone()))
    }
    pub fn tls_acme<EC: Debug, EA: Debug>(
        self,
        config: AcmeConfig<EC, EA>,
    ) -> AcmeIncoming<EC, EA> {
        AcmeIncoming::new(self, config)
    }
    // TODO: add rate limit warning for production
    pub fn tls_lets_encrypt(
        self,
        domains: impl IntoIterator<Item = impl AsRef<str>>,
        contact: impl IntoIterator<Item = impl AsRef<str>>,
        cache_dir: impl AsRef<Path> + Send + Sync + 'static,
        production: bool,
    ) -> AcmeIncoming<io::Error, io::Error> {
        let config = AcmeConfig::new(domains)
            .contact(contact)
            .cache(DirCache::new(cache_dir))
            .directory_lets_encrypt(production);
        self.tls_acme(config)
    }
    pub fn or_tls(self) -> TcpOrTlsIncoming {
        let mut tcp_or_tls = TcpOrTlsIncoming::new();
        tcp_or_tls.push(self);
        tcp_or_tls
    }
    pub fn http(self) -> HttpIncoming<TcpStream, Self> {
        HttpIncoming::new(self)
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.get_ref().local_addr()
    }
}

impl Stream for TcpIncoming {
    type Item = TcpStream;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let listener = self.listener.clone();
        loop {
            match Box::pin(listener.accept()).poll_unpin(cx) {
                Poll::Ready(result) => match result {
                    Ok((stream, _)) => return Poll::Ready(Some(stream.into())),
                    Err(err) => log::debug!("tcp accept error: {:?}", err),
                },
                Poll::Pending => match self.readable.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(_) => self.readable = Box::pin(listener.clone().readable_owned()),
                },
            }
        }
    }
}

impl FusedStream for TcpIncoming {
    fn is_terminated(&self) -> bool {
        false
    }
}
