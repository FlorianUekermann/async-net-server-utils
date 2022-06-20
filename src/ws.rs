use crate::HttpRequest;
use async_http_codec::internal::buffer_write::BufferWrite;
use async_http_codec::ResponseHead;
use async_ws::connection::{WsConfig, WsConnection};
use async_ws::http::upgrade_response;
use futures::prelude::*;
use http::{Request, Response};
use pin_project::pin_project;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

pub enum HttpOrWs<IO: AsyncRead + AsyncWrite + Unpin> {
    Http(HttpRequest<IO>),
    Ws(Request<WsAccept<IO>>),
}

#[pin_project]
pub struct HttpOrWsIncoming<
    IO: AsyncRead + AsyncWrite + Unpin,
    T: Stream<Item = HttpRequest<IO>> + Unpin,
> {
    incoming: T,
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = HttpRequest<IO>> + Unpin>
    HttpOrWsIncoming<IO, T>
{
    pub fn new(http_incoming: T) -> Self {
        Self {
            incoming: http_incoming,
        }
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = HttpRequest<IO>> + Unpin> Stream
    for HttpOrWsIncoming<IO, T>
{
    type Item = HttpOrWs<IO>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        loop {
            return match this.incoming.poll_next_unpin(cx) {
                Poll::Ready(Some(HttpRequest { head, body })) => {
                    let request = head.into();
                    match upgrade_response(&request) {
                        Some(response) => Poll::Ready(Some(HttpOrWs::Ws(Request::from_parts(
                            request.into_parts().0,
                            WsAccept::new(Response::from_parts(
                                response.into_parts().0,
                                body.checkpoint().0,
                            )),
                        )))),
                        None => Poll::Ready(Some(HttpOrWs::Http(HttpRequest {
                            head: request.into(),
                            body,
                        }))),
                    }
                }
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            };
        }
    }
}

pub struct WsAccept<IO: AsyncRead + AsyncWrite + Unpin> {
    response: BufferWrite<IO>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> WsAccept<IO> {
    fn new(response: Response<IO>) -> Self {
        let (head, transport) = response.into_parts();
        Self {
            response: ResponseHead::from(head).encode(transport),
        }
    }
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
