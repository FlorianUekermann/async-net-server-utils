use crate::tcp::TcpIncoming;
use async_net::TcpStream;
use async_rustls::server::TlsStream;
use futures_util::stream::FuturesUnordered;
use futures_util::{AsyncRead, AsyncWrite};
use futures_util::{Stream, StreamExt};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct TlsIncoming<A: TlsAcceptor> {
    tcp_incoming: TcpIncoming,
    tls_acceptor: A,
    accepts: FuturesUnordered<A::Accept<TcpStream>>,
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
    type Accept<IO: AsyncRead + AsyncWrite + Unpin>: Future<
        Output = io::Result<async_rustls::server::TlsStream<IO>>,
    >;

    fn accept<IO: AsyncRead + AsyncWrite + Unpin>(&self, stream: IO) -> Self::Accept<IO>;
}

impl TlsAcceptor for async_rustls::TlsAcceptor {
    type Accept<IO: AsyncRead + AsyncWrite + Unpin> = async_rustls::Accept<IO>;

    fn accept<IO: AsyncRead + AsyncWrite + Unpin>(&self, stream: IO) -> Self::Accept<IO> {
        self.accept(stream)
    }
}

impl TlsAcceptor for rustls_acme::TlsAcceptor {
    type Accept<IO: AsyncRead + AsyncWrite + Unpin> = rustls_acme::Accept<IO>;

    fn accept<IO: AsyncRead + AsyncWrite + Unpin>(&self, stream: IO) -> Self::Accept<IO> {
        self.accept(stream)
    }
}
