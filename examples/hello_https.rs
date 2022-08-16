use async_web_server::{parse_pem, HttpRequest, TcpIncoming, TlsStream};
use clap::Parser;
use futures::prelude::*;
use rcgen::{date_time_ymd, CertificateParams};
use rustls_acme::futures_rustls::rustls::{Certificate, PrivateKey};
use smol::future::block_on;
use smol::spawn;
use std::fs;
use std::net::Ipv6Addr;
use std::path::PathBuf;

#[derive(Parser, Debug)]
struct Args {
    /// Domains
    #[clap(short, required = true)]
    domains: Vec<String>,

    /// HTTP and HTTPS port
    #[clap(short, long, number_of_values = 2, default_values = &["80", "443"])]
    ports: Vec<u16>,

    /// Certificate path
    #[clap(short, long)]
    cert: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Args::parse();

    let (cert_chain, private_key) = match &args.cert {
        None => generate_certificate(args.domains.clone()),
        Some(path) => parse_pem(fs::read(path)?)?,
    };

    let mut incoming = TcpIncoming::bind((Ipv6Addr::UNSPECIFIED, args.ports[1]))?
        .tls(cert_chain, private_key)?
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
    resp.send("Hello HTTPS!").await?;
    log::info!("sent response");
    Ok(())
}

fn generate_certificate(domains: impl Into<Vec<String>>) -> (Vec<Certificate>, PrivateKey) {
    let mut params = CertificateParams::new(domains);
    params.not_before = date_time_ymd(2000, 1, 1);
    params.not_after = date_time_ymd(3000, 1, 1);
    let cert_and_key = rcgen::Certificate::from_params(params).unwrap();
    // let mut file = fs::File::create("./examples/cert.pem").unwrap();
    // file.write_all(cert_and_key.serialize_private_key_pem().as_bytes()).unwrap();
    // file.write_all(cert_and_key.serialize_pem().unwrap().as_bytes()).unwrap();
    // file.sync_all().unwrap();
    (
        vec![Certificate(cert_and_key.serialize_der().unwrap())],
        PrivateKey(cert_and_key.serialize_private_key_der()),
    )
}
