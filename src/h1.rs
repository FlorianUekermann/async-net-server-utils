use crate::{HttpOrWsIncoming, TcpIncoming, TcpStream};
use async_http_codec::internal::buffer_decode::BufferDecode;
use async_http_codec::{BodyDecodeWithContinue, BodyEncode, RequestHead, ResponseHead};
use futures::prelude::*;
use futures::stream::{FusedStream, FuturesUnordered};
use futures::StreamExt;
use http::header::{IntoHeaderName, HOST, LOCATION, TRANSFER_ENCODING};
use http::uri::{Authority, Parts, Scheme};
use http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri, Version};
use log::debug;
use std::borrow::Cow;
use std::convert::TryFrom;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct HttpIncoming<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO> + Unpin> {
    incoming: Option<T>,
    decoding: FuturesUnordered<BufferDecode<IO, RequestHead<'static>>>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO> + Unpin> HttpIncoming<IO, T> {
    pub fn new(transport_incoming: T) -> Self {
        HttpIncoming {
            incoming: Some(transport_incoming),
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
    type Item = HttpRequest<IO>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.decoding.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok((transport, head)))) => {
                    match BodyDecodeWithContinue::from_head(&head, transport) {
                        Ok(body) => return Poll::Ready(Some(HttpRequest { head, body })),
                        Err(err) => log::debug!("http head error: {:?}", err),
                    };
                }
                Poll::Ready(Some(Err(err))) => log::debug!("http head decode error: {:?}", err),
                Poll::Ready(None) | Poll::Pending => match &mut self.incoming {
                    Some(incoming) => match incoming.poll_next_unpin(cx) {
                        Poll::Ready(Some(transport)) => {
                            self.decoding.push(RequestHead::decode(transport))
                        }
                        Poll::Ready(None) => drop(self.incoming.take()),
                        Poll::Pending => return Poll::Pending,
                    },
                    None => match self.is_terminated() {
                        true => return Poll::Ready(None),
                        false => return Poll::Pending,
                    },
                },
            }
        }
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO> + Unpin> FusedStream
    for HttpIncoming<IO, T>
{
    fn is_terminated(&self) -> bool {
        self.incoming.is_none() && self.decoding.is_terminated()
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO> + Unpin> Unpin
    for HttpIncoming<IO, T>
{
}

pub struct HttpRequest<IO: AsyncRead + AsyncWrite + Unpin> {
    pub(crate) head: RequestHead<'static>,
    pub(crate) body: BodyDecodeWithContinue<IO>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> HttpRequest<IO> {
    /// Direct access to the request as [http::Request] with body decoder and underlying transport.
    /// The transport may be extracted using
    /// ```no_run
    /// # use futures::io::Cursor;
    /// # use async_web_server::{HttpRequest};
    /// # let request: HttpRequest<Cursor<&mut [u8]>> = todo!();
    /// let transport = request.into_inner();
    /// ```
    /// Doing this after starting to read the body may lead to one of these situations:
    /// - The meaning of remaining data read from the transport is ambiguous, if the body has been
    /// partially consumed and is encoded using
    /// [chunked transfer encoding](https://en.wikipedia.org/wiki/Chunked_transfer_encoding).
    /// - If the request includes the `Expect: 100-continue` header and if the body reader has been
    /// polled, but did not yield any data yet, the informational response `100 Continue` may have
    /// been fully, partially or not sent on the transport.
    ///
    /// Reasoning about the protocol state is only trivial before calling [Self::body()] and after
    /// consuming the whole body.
    pub fn into_inner(self) -> Request<BodyDecodeWithContinue<IO>> {
        Request::from_parts(self.head.into(), self.body)
    }
    /// The inverse of [Self::into_inner()].
    pub fn from_inner(request: Request<BodyDecodeWithContinue<IO>>) -> Self {
        let (head, body) = request.into_parts();
        let head = head.into();
        Self { head, body }
    }
    /// Move on to responding after consuming and discarding the remaining request body data.
    pub async fn response(mut self) -> io::Result<HttpResponse<IO>> {
        while 0 < self.body().read(&mut [0u8; 1 << 14]).await? {}
        let Self { head, body } = self;
        let request_head = http::request::Parts::from(head);
        let request_headers = request_head.headers;
        let request_method = request_head.method;
        let request_uri = request_head.uri;
        let transport = body.checkpoint().0;
        let headers = Cow::Owned(HeaderMap::with_capacity(128));
        Ok(HttpResponse {
            request_headers,
            request_uri,
            request_method,
            head: ResponseHead::new(StatusCode::OK, request_head.version, headers),
            transport,
        })
    }

    /// Access the request body data stream as [futures::io::AsyncRead].
    /// If the request includes the `Expect: 100-continue` the informational response `100 Continue`
    /// will be sent to the client before the reader emits any body data.
    pub fn body(&mut self) -> &mut BodyDecodeWithContinue<IO> {
        &mut self.body
    }
    /// Read whole body as [String]. See [Self::body] for details.
    pub async fn body_string(&mut self) -> io::Result<String> {
        let mut body = String::new();
        self.body().read_to_string(&mut body).await?;
        Ok(body)
    }
    /// Read whole body as [Vec]. See [Self::body] for details.
    pub async fn body_vec(&mut self) -> io::Result<Vec<u8>> {
        let mut body = Vec::new();
        self.body().read_to_end(&mut body).await?;
        Ok(body)
    }
    /// Access the headers as [http::HeaderMap].
    pub fn headers(&self) -> &HeaderMap {
        self.head.headers()
    }
    /// Access the URI as [http::Uri].
    pub fn uri(&self) -> &Uri {
        &self.head.uri()
    }
    /// Return the method as [http::Method].
    pub fn method(&self) -> Method {
        self.head.method().clone()
    }
    /// Return the HTTP version as [http::Version].
    pub fn version(&self) -> Version {
        self.head.version()
    }
}

pub struct HttpResponse<IO: AsyncRead + AsyncWrite + Unpin> {
    request_uri: Uri,
    request_headers: HeaderMap,
    request_method: Method,
    head: ResponseHead<'static>,
    transport: IO,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> HttpResponse<IO> {
    /// Access the original requests headers as [http::HeaderMap].
    pub fn request_headers(&self) -> &HeaderMap {
        &self.request_headers
    }
    /// Access the original requests uri as [http::Uri].
    pub fn uri(&self) -> &Uri {
        &self.request_uri
    }
    /// Return the original requests method as [http::Method].
    pub fn method(&self) -> Method {
        self.request_method.clone()
    }
    /// Return the HTTP version as [http::Version].
    pub fn version(&self) -> Version {
        self.head.version()
    }
    /// Access the currently set response headers as [http::HeaderMap].
    pub fn headers(&self) -> &HeaderMap {
        self.head.headers()
    }
    /// Access the currently set response headers as mutable [http::HeaderMap].
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        self.head.headers_mut()
    }
    /// Insert a response header (chainable).
    pub fn insert_header(&mut self, key: impl IntoHeaderName, value: HeaderValue) -> &mut Self {
        self.headers_mut().insert(key, value);
        self
    }
    /// Access the currently set response status as [http::StatusCode].
    pub fn status(&self) -> StatusCode {
        self.head.status()
    }
    /// Access the currently set response status as mutable [http::StatusCode].
    pub fn status_mut(&mut self) -> &mut StatusCode {
        self.head.status_mut()
    }
    /// Set the response status (chainable).
    pub fn set_status(&mut self, status: StatusCode) -> &mut Self {
        *self.status_mut() = status;
        self
    }
    /// Send the request with the specified body. See [Self::body] for sending a response with a
    /// streaming body.
    pub async fn send(self, body: impl AsRef<[u8]>) -> io::Result<()> {
        let mut encoder = self.body().await?;
        encoder.write_all(body.as_ref()).await?;
        encoder.close().await?;
        Ok(())
    }
    /// Move on to sending body after sending response head.
    pub async fn body(mut self) -> io::Result<BodyEncode<IO>> {
        self.insert_header(TRANSFER_ENCODING, HeaderValue::from_static("chunked"));
        self.head.encode(&mut self.transport).await?;
        Ok(BodyEncode::new(self.transport, None))
    }
}

impl HttpIncoming<TcpStream, TcpIncoming> {
    pub fn redirect_https(self) -> RedirectHttps {
        RedirectHttps {
            incoming: self,
            redirecting: FuturesUnordered::new(),
        }
    }
}

pub struct RedirectHttps {
    incoming: HttpIncoming<TcpStream, TcpIncoming>,
    // TODO: replace Box<dyn ...> once https://github.com/rust-lang/rust/issues/63063 is stable
    redirecting: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>,
}

impl RedirectHttps {
    fn set_location_header(resp: &mut HttpResponse<TcpStream>) -> http::Result<()> {
        let authority = match resp.request_headers().get(HOST) {
            Some(host) => Some(Authority::try_from(host.as_bytes())?),
            None => None,
        };
        let mut parts: Parts = Default::default();
        parts.scheme = Some(Scheme::HTTPS);
        parts.authority = authority;
        parts.path_and_query = resp.uri().path_and_query().cloned();
        let header_value = HeaderValue::try_from(Uri::from_parts(parts)?.to_string())?;
        resp.insert_header(LOCATION, header_value);
        Ok(())
    }
    async fn send(req: HttpRequest<TcpStream>) {
        match req.response().await {
            Err(err) => debug!("error reading body of request to be redirected: {:?}", err),
            Ok(mut resp) => match Self::set_location_header(&mut resp) {
                Err(err) => debug!("error constructing redirect location header: {:?}", err),
                Ok(()) => {
                    resp.set_status(StatusCode::TEMPORARY_REDIRECT);
                    if let Err(err) = resp.send(&[]).await {
                        debug!("error sending redirect response: {:?}", err)
                    }
                }
            },
        }
    }
}

impl Future for RedirectHttps {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            if let Poll::Ready(Some(())) = Pin::new(&mut self.redirecting).poll_next(cx) {
                continue;
            }
            if !self.incoming.is_terminated() {
                if let Poll::Ready(Some(req)) = Pin::new(&mut self.incoming).poll_next(cx) {
                    self.redirecting.push(Box::pin(Self::send(req)));
                    continue;
                }
            }
            return match self.incoming.is_terminated() && self.redirecting.is_terminated() {
                true => Poll::Ready(()),
                false => Poll::Pending,
            };
        }
    }
}
