use crate::{HttpRequest, IsTls, TcpOrTlsIncoming, TcpOrTlsStream};
use async_http_codec::internal::buffer_write::BufferWrite;
use async_http_codec::{RequestHead, ResponseHead};
use async_ws::connection::{WsConfig, WsConnection};
use async_ws::http::{is_upgrade_request, upgrade_response};
use futures::prelude::*;
use futures::stream::FusedStream;
use http::{HeaderMap, Method, Request, Uri, Version};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

pub enum HttpOrWs<IO: AsyncRead + AsyncWrite + Unpin> {
    Http(HttpRequest<IO>),
    Ws(WsUpgradeRequest<IO>),
}

impl<IO: AsyncRead + AsyncWrite + Unpin + IsTls> IsTls for HttpOrWs<IO> {
    fn is_tls(&self) -> bool {
        match self {
            HttpOrWs::Http(http) => http.is_tls(),
            HttpOrWs::Ws(ws) => ws.is_tls(),
        }
    }
}

pub struct HttpOrWsIncoming<
    IO: AsyncRead + AsyncWrite + Unpin = TcpOrTlsStream,
    T: Stream<Item = HttpRequest<IO>> + Unpin = TcpOrTlsIncoming,
> {
    incoming: Option<T>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = HttpRequest<IO>> + Unpin>
    HttpOrWsIncoming<IO, T>
{
    pub fn new(http_incoming: T) -> Self {
        Self {
            incoming: Some(http_incoming),
        }
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = HttpRequest<IO>> + Unpin> Stream
    for HttpOrWsIncoming<IO, T>
{
    type Item = HttpOrWs<IO>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let incoming = match &mut self.incoming {
            None => return Poll::Ready(None),
            Some(incoming) => incoming,
        };

        let request = match incoming.poll_next_unpin(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => {
                drop(self.incoming.take());
                return Poll::Ready(None);
            }
            Poll::Ready(Some(request)) => request,
        };

        let request = request.into_inner();
        if !is_upgrade_request(&request) {
            return Poll::Ready(Some(HttpOrWs::Http(HttpRequest::from_inner(request))));
        }

        let response = upgrade_response(&request).unwrap();
        let (request_head, request_body) = request.into_parts();
        let request_head = RequestHead::from(request_head);
        let (_, transport) = request_body.into_inner();
        let response_head = ResponseHead::from(response);
        Poll::Ready(Some(HttpOrWs::Ws(WsUpgradeRequest {
            request_head,
            response_head,
            transport,
        })))
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = HttpRequest<IO>> + Unpin> FusedStream
    for HttpOrWsIncoming<IO, T>
{
    fn is_terminated(&self) -> bool {
        self.incoming.is_none()
    }
}

pub struct WsUpgradeRequest<IO: AsyncRead + AsyncWrite + Unpin> {
    pub(crate) request_head: RequestHead<'static>,
    pub(crate) response_head: ResponseHead<'static>,
    pub(crate) transport: IO,
}

impl<IO: AsyncRead + AsyncWrite + Unpin + IsTls> IsTls for WsUpgradeRequest<IO> {
    fn is_tls(&self) -> bool {
        self.transport.is_tls()
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> WsUpgradeRequest<IO> {
    /// Direct access to the request as [http::Request] and underlying transport.
    /// The transport may be extracted using
    /// ```no_run
    /// # use futures::io::Cursor;
    /// # use async_web_server::WsUpgradeRequest;
    /// # let request: WsUpgradeRequest<Cursor<&mut [u8]>> = todo!();
    /// let transport = request.into_inner();
    /// ```
    pub fn into_inner(self) -> Request<IO> {
        Request::from_parts(self.request_head.into(), self.transport)
    }
    /// Access the original requests headers as [http::HeaderMap].
    pub fn request_headers(&self) -> &HeaderMap {
        self.request_head.headers()
    }
    /// Access the original requests URI as [http::Uri].
    pub fn uri(&self) -> &Uri {
        &self.request_head.uri()
    }
    /// Return the original requests method as [http::Method].
    pub fn method(&self) -> Method {
        self.request_head.method().clone()
    }
    /// Return the HTTP version as [http::Version].
    pub fn version(&self) -> Version {
        self.request_head.version()
    }
    /// Upgrade to a websocket connection.
    pub fn upgrade(self) -> WsAccept<IO> {
        WsAccept {
            response: self.response_head.encode(self.transport),
        }
    }
}

pub struct WsAccept<IO: AsyncRead + AsyncWrite + Unpin> {
    response: BufferWrite<IO>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> Future for WsAccept<IO> {
    type Output = io::Result<WsConnection<IO>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.response.poll_unpin(cx) {
            Poll::Ready(Ok(transport)) => {
                Poll::Ready(Ok(WsConnection::with_config(transport, WsConfig::server())))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}
