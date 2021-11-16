use async_http_codec::head::decode::{RequestHeadDecode, RequestHeadDecoder};
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use http::Request;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct HttpIncoming<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO>> {
    incoming: T,
    decoding: FuturesUnordered<RequestHeadDecode<IO>>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO>> HttpIncoming<IO, T> {
    pub fn new(transport_incoming: T) -> Self {
        HttpIncoming {
            incoming: transport_incoming,
            decoding: FuturesUnordered::new(),
        }
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO> + Unpin> Stream
    for HttpIncoming<IO, T>
{
    type Item = (IO, Request<()>);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            dbg!();
            match self.decoding.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(request))) => {
                    dbg!("decoded");
                    return Poll::Ready(Some(request));
                }
                Poll::Ready(Some(Err(err))) => log::error!("http decoding error: {:?}", err),
                Poll::Ready(None) | Poll::Pending => match self.incoming.poll_next_unpin(cx) {
                    Poll::Ready(Some(transport)) => {
                        dbg!("add");
                        self.decoding
                            .push(RequestHeadDecoder::default().decode(transport))
                    }
                    Poll::Ready(None) => return Poll::Ready(None),
                    Poll::Pending => {
                        dbg!("pending");
                        return Poll::Pending;
                    }
                },
            }
        }
    }
}
