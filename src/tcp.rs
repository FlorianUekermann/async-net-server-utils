use crate::http::HttpIncoming;
use crate::tls::{TlsAcceptor, TlsIncoming};
use async_io::{Async, ReadableOwned};
use async_net::TcpStream;
use futures::prelude::*;
use futures::FutureExt;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

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
    pub fn tls<A: TlsAcceptor>(self, tls_acceptor: A) -> TlsIncoming<A> {
        TlsIncoming::new(self, tls_acceptor)
    }
    pub fn http(self) -> HttpIncoming<TcpStream, Self> {
        HttpIncoming::new(self)
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
                    Err(err) => log::error!("tcp accept error: {:?}", err),
                },
                Poll::Pending => match self.readable.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(_) => self.readable = Box::pin(listener.clone().readable_owned()),
                },
            }
        }
    }
}
