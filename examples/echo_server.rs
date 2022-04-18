use anyhow::{bail, Context};
use async_global_executor::{block_on, spawn};
use async_http_codec::{BodyDecode, BodyEncode, ResponseHeadEncoder};
use async_web_server::tcp::{TcpIncoming, TcpStream};
use async_web_server::ws::{HttpOrWs, WsAccept};
use futures::io::copy;
use futures::prelude::*;
use http::header::TRANSFER_ENCODING;
use http::{Method, Request, Response};
use log::LevelFilter;
use simple_logger::SimpleLogger;
use std::convert::TryInto;
use std::net::Ipv4Addr;

const HTML: &[u8] = include_bytes!("echo-client.html");

fn main() -> anyhow::Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    let mut incoming = TcpIncoming::bind((Ipv4Addr::UNSPECIFIED, 8080))?
        .http()
        .or_ws();

    block_on(async {
        while let Some(req) = incoming.next().await {
            spawn(async {
                let result = match req {
                    HttpOrWs::Http(req) => handle_http(req).await.context("http"),
                    HttpOrWs::Ws(req) => handle_ws(req).await.context("ws"),
                };
                if let Err(err) = result {
                    log::error!("error handling request: {:?}", err);
                }
            })
            .detach();
        }
        unreachable!()
    })
}

async fn handle_http(req: Request<BodyDecode<TcpStream>>) -> anyhow::Result<()> {
    let (req_head, mut req_body_read) = req.into_parts();
    log::info!(
        "received {:?} request head on {:?}",
        req_head.method,
        req_head.uri
    );

    let mut req_body = Vec::new();
    req_body_read.read_to_end(&mut req_body).await?;
    let mut transport = req_body_read.checkpoint().0;
    log::info!("received request body with {} bytes", req_body.len());

    let mut resp_head = Response::new(()).into_parts().0;
    resp_head
        .headers
        .insert(TRANSFER_ENCODING, "chunked".try_into()?);
    ResponseHeadEncoder::default()
        .encode(&mut transport, &resp_head)
        .await?;
    log::info!("sent response head");

    let resp_body = match req_head.method {
        Method::GET => HTML,
        Method::POST => &req_body,
        _ => bail!("unexpected request method"),
    };
    let mut resp_body_write = BodyEncode::from_headers(&resp_head.headers, &mut transport)?;
    resp_body_write.write_all(resp_body).await?;
    resp_body_write.close().await?;
    log::info!("sent response body");

    Ok(())
}

async fn handle_ws(req: Request<WsAccept<TcpStream>>) -> anyhow::Result<()> {
    log::info!("received websocket upgrade request on {:?}", req.uri());
    let mut ws = req.into_body().await?;
    log::info!("accepted websocket handshake");

    while let Some(mut msg_read) = ws.next().await {
        let mut msg_write = ws
            .send(msg_read.kind())
            .await
            .context("closed unexpectedly")?;
        let n = copy(&mut msg_read, &mut msg_write).await?;
        msg_write.close().await?;
        log::info!(
            "echoed websocket {:?} message with {} bytes",
            msg_read.kind(),
            n
        );
    }

    log::info!("websocket closed: {:?}", ws.err());
    Ok(())
}
