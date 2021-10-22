use async_tungstenite::tungstenite::handshake::server::create_response;
use async_tungstenite::WebSocketStream;
use futures::io::BufReader;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use http::{Request, Response};
use std::pin::Pin;
use std::task::{Context, Poll};
use crate::http::{http_response_write, HttpEncode};
use futures::FutureExt;
use async_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};

pub enum WsOrHttp<IO: AsyncRead + AsyncWrite + Unpin> {
    Http(Request<BufReader<IO>>),
    Ws(Request<BufReader<IO>>),
}

pub struct WsOrHttpIncoming<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = Request<IO>>> {
    incoming: T,
    accepts: FuturesUnordered<WsAccept<IO>>,
}

pub fn ws_accept<IO: AsyncRead + AsyncWrite + Unpin>(request: Request<IO>) -> WsAccept<IO> {
    let (parts, transport) = request.into_parts();
    let response = create_response(&Request::from_parts(parts, ()))
     .map(|response| Response::from_parts(response.into_parts().0, transport))
     .map(http_response_write)
        .map_err(|err| anyhow::Error::new(err));
    WsAccept::<IO> { response: Some(response) }
}

pub struct WsAccept<IO: AsyncRead + AsyncWrite + Unpin> {
    response: Option<anyhow::Result<HttpEncode<IO>>>
}

impl<IO: AsyncRead + AsyncWrite + Unpin> Future for WsAccept<IO> {
    type Output = anyhow::Result<WebSocketStream<BufReader<IO>>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut response = match self.response.take() {
            Some(response) => response,
            None => panic!("future polled after returning ready"),
        };
        match response {
            Ok(mut encode) => match encode.poll_unpin(cx) {
                Poll::Ready(Ok(transport)) => Poll::Ready(Ok(WebSocketStream::from_raw_socket(
                    transport,
                    Role::Server,
                    Some(WebSocketConfig{
                        max_send_queue: Some(1),
                        max_message_size: Some(64 << 20),
                        max_frame_size: Some(16 << 20),
                        accept_unmasked_frames: false
                    })
                ))),
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                Poll::Pending => {
                    self.response = Some(response);
                    Poll::Pending
                }
            }
            Err(_) => {}
        }
    }
}
