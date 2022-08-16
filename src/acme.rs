use crate::tcp::TcpIncoming;
use crate::{TcpStream, TlsStream};
use futures::prelude::*;
use futures::stream::FusedStream;
use futures::StreamExt;
use rustls_acme::AcmeConfig;
use std::convert::Infallible;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct AcmeIncoming<EC: Debug + 'static, EA: Debug + 'static> {
    incoming: rustls_acme::Incoming<TcpStream, Infallible, TcpIncomingInfallible, EC, EA>,
}

impl<EC: Debug, EA: Debug> AcmeIncoming<EC, EA> {
    pub fn new(tcp_incoming: TcpIncoming, config: AcmeConfig<EC, EA>) -> Self {
        let incoming = config.incoming(TcpIncomingInfallible(tcp_incoming));
        AcmeIncoming { incoming }
    }
}

impl<EC: Debug, EA: Debug> Stream for AcmeIncoming<EC, EA> {
    type Item = TlsStream;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.incoming.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok(item))) => Poll::Ready(Some(item)),
            Poll::Ready(Some(Err(_))) => unreachable!(),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<EC: Debug, EA: Debug> FusedStream for AcmeIncoming<EC, EA> {
    fn is_terminated(&self) -> bool {
        self.incoming.is_terminated()
    }
}

struct TcpIncomingInfallible(TcpIncoming);

impl Stream for TcpIncomingInfallible {
    type Item = Result<TcpStream, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.0.poll_next_unpin(cx) {
            Poll::Ready(Some(tcp)) => Poll::Ready(Some(Ok(tcp))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl FusedStream for TcpIncomingInfallible {
    fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
}
