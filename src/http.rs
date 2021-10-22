use anyhow::{anyhow, bail};
use futures_lite::AsyncBufReadExt;
use futures_util::io::BufReader;
use futures_util::stream::FuturesUnordered;
use futures_util::{AsyncBufRead, AsyncRead, AsyncWrite, Stream, StreamExt};
use http::{header::HeaderName, HeaderValue, Method, Request, Uri, Version};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct HttpIncoming<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = IO>> {
    incoming: T,
    decoding: FuturesUnordered<HttpDecode<IO>>,
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
    type Item = Request<BufReader<IO>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.decoding.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(request))) => return Poll::Ready(Some(request)),
                Poll::Ready(Some(Err(err))) => log::error!("http decoding error: {:?}", err),
                Poll::Ready(None) | Poll::Pending => match self.incoming.poll_next_unpin(cx) {
                    Poll::Ready(Some(transport)) => self.decoding.push(http_accept(transport)),
                    Poll::Ready(None) => return Poll::Ready(None),
                    Poll::Pending => return Poll::Pending,
                },
            }
        }
    }
}

pub struct HttpDecode<IO: AsyncRead + AsyncWrite + Unpin> {
    transport: Option<BufReader<IO>>,
    buffer: [u8; 8192],
    len: usize,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> Future for HttpDecode<IO> {
    type Output = anyhow::Result<Request<BufReader<IO>>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut transport = match self.transport.take() {
            Some(transport) => transport,
            None => panic!("future polled after returning ready"),
        };
        loop {
            match Pin::new(&mut transport).poll_fill_buf(cx) {
                Poll::Ready(Ok(_)) => {
                    while transport.buffer().len() > 0 {
                        let n = match transport.buffer().iter().position(|&x| x == b'\n') {
                            Some(pos) => pos + 1,
                            None => transport.buffer().len(),
                        };
                        if self.buffer.len() < self.len + n {
                            return Poll::Ready(Err(anyhow!("HTTP head too long")));
                        }
                        let off = self.len;
                        self.buffer[off..off + n].copy_from_slice(&transport.buffer()[0..n]);
                        self.len += n;
                        transport.consume(n);
                        if self.buffer[0..self.len].ends_with(b"\r\n\r\n") {
                            match http_request_parse(&self.buffer[0..self.len], transport) {
                                Ok(request) => return Poll::Ready(Ok(request)),
                                Err(err) => return Poll::Ready(Err(err)),
                            }
                        }
                    }
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err.into())),
                Poll::Pending => {
                    self.transport = Some(transport);
                    return Poll::Pending;
                }
            };
        }
    }
}

pub fn http_accept<IO: AsyncRead + AsyncWrite + Unpin>(io: IO) -> HttpDecode<IO> {
    HttpDecode {
        transport: Some(BufReader::new(io)),
        buffer: [0u8; 8192],
        len: 0,
    }
}

pub fn http_request_parse<T>(buffer: &[u8], body: T) -> anyhow::Result<Request<T>> {
    let mut headers = [httparse::EMPTY_HEADER; 128];
    let mut parsed_request = httparse::Request::new(&mut headers);
    if parsed_request.parse(buffer)?.is_partial() {
        bail!("invalid HTTP head")
    }
    if parsed_request.version != Some(1) {
        bail!("unsupported HTTP version")
    }
    let method = Method::from_bytes(parsed_request.method.unwrap_or("").as_bytes())?;
    let uri = parsed_request.path.unwrap_or("").parse::<Uri>()?;
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .version(Version::HTTP_11)
        .body(body)?;
    let headers = request.headers_mut();
    headers.reserve(parsed_request.headers.len());
    for header in parsed_request.headers {
        headers.append(
            HeaderName::from_bytes(header.name.as_bytes())?,
            HeaderValue::from_bytes(header.value)?,
        );
    }
    Ok(request)
}
