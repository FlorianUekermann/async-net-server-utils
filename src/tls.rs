use crate::tcp::TcpIncoming;
use async_net::TcpStream;
use async_rustls::server::TlsStream;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use std::io;
use std::pin::Pin;

use futures::StreamExt;
use std::task::{Context, Poll};

pub struct TlsIncoming<A: TlsAcceptor> {
    tcp_incoming: TcpIncoming,
    tls_acceptor: A,
    accepts: FuturesUnordered<A::Accept>,
}

impl<A: TlsAcceptor> TlsIncoming<A> {
    pub fn new(tcp_incoming: TcpIncoming, tls_acceptor: A) -> Self {
        let accepts = FuturesUnordered::new();
        TlsIncoming {
            tcp_incoming,
            tls_acceptor,
            accepts,
        }
    }
}

impl<A: TlsAcceptor> Stream for TlsIncoming<A> {
    type Item = TlsStream<TcpStream>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.accepts.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(tls_stream))) => return Poll::Ready(Some(tls_stream)),
                Poll::Ready(Some(Err(err))) => log::error!("tls accept error: {:?}", err),
                Poll::Ready(None) | Poll::Pending => match self.tcp_incoming.poll_next_unpin(cx) {
                    Poll::Ready(Some(tcp_stream)) => {
                        self.accepts.push(self.tls_acceptor.accept(tcp_stream))
                    }
                    Poll::Ready(None) => return Poll::Ready(None),
                    Poll::Pending => return Poll::Pending,
                },
            }
        }
    }
}

pub trait TlsAcceptor: Unpin {
    type Accept: Future<Output = io::Result<TlsStream<TcpStream>>>;

    fn accept(&self, stream: TcpStream) -> Self::Accept;
}

impl TlsAcceptor for async_rustls::TlsAcceptor {
    type Accept = async_rustls::Accept<TcpStream>;

    fn accept(&self, stream: TcpStream) -> Self::Accept {
        self.accept(stream)
    }
}

impl TlsAcceptor for rustls_acme::TlsAcceptor {
    type Accept = rustls_acme::Accept<TcpStream>;

    fn accept(&self, stream: TcpStream) -> Self::Accept {
        self.accept(stream)
    }
}
