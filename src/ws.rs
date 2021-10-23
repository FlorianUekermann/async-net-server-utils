use async_http_codec::head::decode::RequestHeadDecode;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use http::Request;

pub enum WsOrHttp<IO: AsyncRead + AsyncWrite + Unpin> {
    Http(Request<IO>),
    Ws(Request<IO>),
}

pub struct WsOrHttpIncoming<IO: AsyncRead + AsyncWrite + Unpin, T: Stream<Item = Request<IO>>> {
    _incoming: T,
    _upgrading: FuturesUnordered<RequestHeadDecode<IO, IO>>,
}
