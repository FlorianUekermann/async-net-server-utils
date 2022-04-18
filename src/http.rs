use crate::ws::HttpOrWsIncoming;
use async_http_codec::{BodyDecode, RequestHeadDecode, RequestHeadDecoder};
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use http::Request;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct HttpIncoming<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO> + Unpin> {
    incoming: T,
    decoding: FuturesUnordered<RequestHeadDecode<IO>>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO> + Unpin> HttpIncoming<IO, T> {
    pub fn new(transport_incoming: T) -> Self {
        HttpIncoming {
            incoming: transport_incoming,
            decoding: FuturesUnordered::new(),
        }
    }
    pub fn or_ws(self) -> HttpOrWsIncoming<IO, Self> {
        HttpOrWsIncoming::new(self)
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO> + Unpin> Stream
    for HttpIncoming<IO, T>
{
    type Item = Request<BodyDecode<IO>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.decoding.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok((transport, head)))) => {
                    match BodyDecode::from_headers(&head.headers, transport) {
                        Ok(body) => return Poll::Ready(Some(Request::from_parts(head, body))),
                        Err(err) => log::error!("http head error: {:?}", err),
                    };
                }
                Poll::Ready(Some(Err(err))) => log::error!("http head decode error: {:?}", err),
                Poll::Ready(None) | Poll::Pending => match self.incoming.poll_next_unpin(cx) {
                    Poll::Ready(Some(transport)) => self
                        .decoding
                        .push(RequestHeadDecoder::default().decode(transport)),
                    Poll::Ready(None) => return Poll::Ready(None),
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                },
            }
        }
    }
}
