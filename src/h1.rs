use crate::ws::HttpOrWsIncoming;
use async_http_codec::{BodyDecode, BodyDecodeWithContinue, RequestHeadDecode, RequestHeadDecoder};
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use http::request::Parts;
use http::{HeaderMap, Method, Request, Uri, Version};
use std::marker::PhantomData;
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

pub struct HttpRequestReceiving<IO: AsyncRead + AsyncWrite + Unpin> {
    head: Parts,
    decode: BodyDecodeWithContinue<IO>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> HttpRequestReceiving<IO> {
    /// Direct access to the request and underlying transport for the body.
    /// Calling this method after starting to read the body may lead to one of these situations:
    /// - The meaning of remaining data read from the transport is ambiguous, if the body has been
    /// partially consumed and is encoded using
    /// [chunked transfer encoding](https://en.wikipedia.org/wiki/Chunked_transfer_encoding).
    /// - If the request includes the `Expect: 100-continue` header and if the body reader has been
    /// polled, but did not yield any data yet, the informational response `100 Continue` may have
    /// been fully, partially or not sent on the transport.
    ///
    /// Reasoning about the protocol state is only trivial before calling [Self::body()] and after
    /// consuming the whole body.
    pub fn into_request(self) -> Request<IO> {
        todo!("implement checkpointing for BodyDecodeWithContinue")
        // Request::from_parts(self.head, self.decode.checkpoint().0)
    }
    /// Move on to responding after consuming and discarding the remaining request body data.
    pub async fn respond(&mut self) -> anyhow::Result<HttpResponseSending<IO>> {
        while 0 < self.body().read(&mut [0u8; 1 << 14]).await? {}
        Ok(HttpResponseSending {
            _p: Default::default(),
        })
    }
    /// Return [futures::io::AsyncRead] for the body data.
    /// If the request includes the `Expect: 100-continue` the informational response `100 Continue`
    /// will be sent to the client before the reader emits any body data.
    pub fn body(&mut self) -> &mut BodyDecodeWithContinue<IO> {
        &mut self.decode
    }
    /// Access the head as [http::request::Parts].
    pub fn head(&self) -> &Parts {
        &self.head
    }
    /// Access the headers as [http::HeaderMap].
    pub fn headers(&self) -> &HeaderMap {
        &self.head.headers
    }
    /// Access the URI as [http::Uri].
    pub fn uri(&self) -> &Uri {
        &self.head.uri
    }
    /// Return the method as [http::Method].
    pub fn method(&self) -> Method {
        self.head.method.clone()
    }
    /// Return the HTTP version as [http::Version].
    pub fn version(&self) -> Version {
        self.head.version
    }
}

pub struct HttpResponseSending<IO: AsyncRead + AsyncWrite + Unpin> {
    _p: PhantomData<IO>,
}
