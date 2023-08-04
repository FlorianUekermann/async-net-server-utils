use crate::{HttpIncoming, TcpStream, TlsStream};
use futures::prelude::*;
use futures::stream::{FusedStream, SelectAll};
use futures::StreamExt;
use std::pin::Pin;
use std::task::{Context, Poll};

pub trait IsTls {
    fn is_tls(&self) -> bool;
}

pub enum TcpOrTlsStream {
    Tcp(TcpStream),
    Tls(TlsStream),
}

impl IsTls for TcpOrTlsStream {
    fn is_tls(&self) -> bool {
        match self {
            Self::Tcp(_) => false,
            Self::Tls(_) => true,
        }
    }
}

pub struct TcpOrTlsIncoming {
    incomings: SelectAll<Box<dyn Stream<Item = TcpOrTlsStream> + Unpin>>,
}

impl TcpOrTlsIncoming {
    pub fn new() -> Self {
        Self {
            incomings: SelectAll::new(),
        }
    }
    pub fn push(
        &mut self,
        incoming: impl Stream<Item = impl Into<TcpOrTlsStream>> + Unpin + 'static,
    ) {
        self.incomings
            .push(Box::new(incoming.map(|stream| stream.into())))
    }
    pub fn merge(&mut self, other: Self) {
        self.incomings.extend(other.incomings.into_iter())
    }
    pub fn http(self) -> HttpIncoming<TcpOrTlsStream, Self> {
        HttpIncoming::new(self)
    }
}

impl Unpin for TcpOrTlsIncoming {}

impl Stream for TcpOrTlsIncoming {
    type Item = TcpOrTlsStream;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.incomings.poll_next_unpin(cx)
    }
}

impl FusedStream for TcpOrTlsIncoming {
    fn is_terminated(&self) -> bool {
        self.incomings.is_terminated()
    }
}

impl AsyncRead for TcpOrTlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            TcpOrTlsStream::Tcp(tcp) => Pin::new(tcp).poll_read(cx, buf),
            TcpOrTlsStream::Tls(tls) => Pin::new(tls).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TcpOrTlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            TcpOrTlsStream::Tcp(tcp) => Pin::new(tcp).poll_write(cx, buf),
            TcpOrTlsStream::Tls(tls) => Pin::new(tls).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TcpOrTlsStream::Tcp(tcp) => Pin::new(tcp).poll_flush(cx),
            TcpOrTlsStream::Tls(tls) => Pin::new(tls).poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TcpOrTlsStream::Tcp(tcp) => Pin::new(tcp).poll_close(cx),
            TcpOrTlsStream::Tls(tls) => Pin::new(tls).poll_close(cx),
        }
    }
}

impl From<TcpStream> for TcpOrTlsStream {
    fn from(value: TcpStream) -> Self {
        TcpOrTlsStream::Tcp(value)
    }
}

impl From<TlsStream> for TcpOrTlsStream {
    fn from(value: TlsStream) -> Self {
        TcpOrTlsStream::Tls(value)
    }
}
