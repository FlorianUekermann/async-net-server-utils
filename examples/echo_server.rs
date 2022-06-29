use anyhow::{bail, Context};
use async_web_server::{HttpOrWs, HttpRequest, TcpIncoming, TcpStream, WsUpgradeRequest};
use futures::io::copy;
use futures::prelude::*;
use http::Method;
use log::LevelFilter;
use simple_logger::SimpleLogger;
use smol::future::block_on;
use smol::spawn;
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

async fn handle_http(mut req: HttpRequest<TcpStream>) -> anyhow::Result<()> {
    log::info!(
        "received {:?} request head on {:?}",
        req.method(),
        req.uri()
    );

    let body = req.body_string().await?;
    log::info!("received request body with {} bytes", body.len());

    let resp = req.response().await?;
    match resp.method() {
        Method::GET => resp.send(HTML).await?,
        Method::POST => resp.send(body).await?,
        _ => bail!("unexpected request method"),
    }
    log::info!("sent response body");
    Ok(())
}

async fn handle_ws(req: WsUpgradeRequest<TcpStream>) -> anyhow::Result<()> {
    log::info!("received websocket upgrade request on {:?}", req.uri());
    let mut ws = req.upgrade().await?;
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
