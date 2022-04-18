use async_http_codec::{BodyDecode, ResponseHeadEncode, ResponseHeadEncoder};
use async_ws::connection::{WsConfig, WsConnection};
use async_ws::http::upgrade_response;
use futures::prelude::*;
use http::{response, Request, Response};
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

pub enum HttpOrWs<IO: AsyncRead + AsyncWrite + Unpin> {
    Http(Request<BodyDecode<IO>>),
    Ws(Request<WsAccept<IO>>),
}

#[pin_project]
pub struct HttpOrWsIncoming<
    IO: AsyncRead + AsyncWrite + Unpin,
    T: Stream<Item = Request<BodyDecode<IO>>> + Unpin,
> {
    incoming: T,
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = Request<BodyDecode<IO>>> + Unpin>
    HttpOrWsIncoming<IO, T>
{
    pub fn new(http_incoming: T) -> Self {
        Self {
            incoming: http_incoming,
        }
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = Request<BodyDecode<IO>>> + Unpin> Stream
    for HttpOrWsIncoming<IO, T>
{
    type Item = HttpOrWs<IO>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        loop {
            return match this.incoming.poll_next_unpin(cx) {
                Poll::Ready(Some(request)) => match upgrade_response(&request) {
                    Some(response) => {
                        let (request_head, body_read) = request.into_parts();
                        let response =
                            Response::from_parts(response.into_parts().0, body_read.checkpoint().0);
                        Poll::Ready(Some(HttpOrWs::Ws(Request::from_parts(
                            request_head,
                            WsAccept::new(response),
                        ))))
                    }
                    None => Poll::Ready(Some(HttpOrWs::Http(request))),
                },
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            };
        }
    }
}

pub struct WsAccept<IO: AsyncRead + AsyncWrite + Unpin> {
    response: ResponseHeadEncode<IO, response::Parts>,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> WsAccept<IO> {
    fn new(response: Response<IO>) -> Self {
        let (head, transport) = response.into_parts();
        Self {
            response: ResponseHeadEncoder::default().encode(transport, head),
        }
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> Future for WsAccept<IO> {
    type Output = anyhow::Result<WsConnection<IO>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.response.poll_unpin(cx) {
            Poll::Ready(Ok((transport, _))) => {
                Poll::Ready(Ok(WsConnection::with_config(transport, WsConfig::server())))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }
}
