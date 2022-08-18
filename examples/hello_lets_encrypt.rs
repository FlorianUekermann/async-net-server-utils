use async_web_server::{HttpRequest, TcpIncoming, TlsStream};
use clap::Parser;
use futures::prelude::*;
use smol::future::block_on;
use smol::spawn;
use std::net::Ipv6Addr;
use std::path::PathBuf;

#[derive(Parser, Debug)]
struct Args {
    /// Domains
    #[clap(short, long, required = true)]
    domains: Vec<String>,

    /// Contact info
    #[clap(short, long)]
    email: Vec<String>,

    /// Cache directory
    #[clap(short, long, parse(from_os_str))]
    cache: PathBuf,

    /// HTTP and HTTPS port
    #[clap(short, long, number_of_values = 2, default_values = &["80", "443"])]
    ports: Vec<u16>,

    /// Use Let's Encrypt production environment
    /// (see https://letsencrypt.org/docs/staging-environment/)
    #[clap(long)]
    prod: bool,
}

fn main() -> anyhow::Result<()> {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Args::parse();

    let redirect_http = TcpIncoming::bind((Ipv6Addr::UNSPECIFIED, args.ports[0]))?
        .http()
        .redirect_https();
    spawn(redirect_http).detach();

    let contact = args.email.iter().map(|e| format!("mailto:{}", e));

    let mut incoming = TcpIncoming::bind((Ipv6Addr::UNSPECIFIED, args.ports[1]))?
        .tls_lets_encrypt(args.domains, contact, args.cache, args.prod)
        .http();

    block_on(async {
        while let Some(req) = incoming.next().await {
            spawn(async {
                if let Err(err) = handle_http(req).await {
                    log::error!("error handling request: {:?}", err);
                }
            })
            .detach();
        }
        unreachable!()
    })
}

async fn handle_http(req: HttpRequest<TlsStream>) -> anyhow::Result<()> {
    log::info!("received {:?} request head", req.method());
    let resp = req.response().await?;
    log::info!("read request body",);
    resp.send("Hello Let's Encrypt!").await?;
    log::info!("sent response");
    Ok(())
}
