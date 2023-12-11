use async_web_server::{parse_pem, HttpRequest, IsTls, TcpIncoming};
use clap::Parser;
use futures::prelude::*;
use rcgen::{date_time_ymd, CertificateParams};
use rustls_acme::futures_rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
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

    let mut tcp_or_tls = TcpIncoming::bind((Ipv6Addr::UNSPECIFIED, args.ports[1]))?
        .tls(cert_chain, private_key)?
        .or_tcp();
    tcp_or_tls.push(TcpIncoming::bind((Ipv6Addr::UNSPECIFIED, args.ports[0]))?);
    let mut incoming = tcp_or_tls.http();

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

async fn handle_http(req: HttpRequest) -> anyhow::Result<()> {
    let is_tls = req.is_tls();
    log::info!("received {:?} request head", req.method());
    let resp = req.response().await?;
    log::info!("read request body",);
    match is_tls {
        true => resp.send("Hello HTTPS!").await?,
        false => resp.send("Hello HTTP!").await?,
    }
    log::info!("sent response");
    Ok(())
}

fn generate_certificate(
    domains: impl Into<Vec<String>>,
) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    let mut params = CertificateParams::new(domains);
    params.not_before = date_time_ymd(2000, 1, 1);
    params.not_after = date_time_ymd(3000, 1, 1);
    let cert_and_key = rcgen::Certificate::from_params(params).unwrap();
    // let mut file = fs::File::create("./examples/cert.pem").unwrap();
    // file.write_all(cert_and_key.serialize_private_key_pem().as_bytes()).unwrap();
    // file.write_all(cert_and_key.serialize_pem().unwrap().as_bytes()).unwrap();
    // file.sync_all().unwrap();
    (
        vec![CertificateDer::from(cert_and_key.serialize_der().unwrap())],
        PrivatePkcs8KeyDer::from(cert_and_key.serialize_private_key_der()).into(),
    )
}
