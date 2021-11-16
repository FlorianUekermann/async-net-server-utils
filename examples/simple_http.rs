use async_net_server_utils::tcp::TcpIncoming;
use futures::executor::block_on;
use futures::prelude::*;
use log::LevelFilter;
use simple_logger::SimpleLogger;
use std::net::Ipv4Addr;

fn main() -> anyhow::Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .init()
        .unwrap();

    block_on(async {
        let mut cnt = 0;

        let mut incoming = TcpIncoming::bind((Ipv4Addr::UNSPECIFIED, 8080))?.http();
        while let Some(request) = incoming.next().await {
            cnt += 1;
            dbg!(request, cnt);
        }

        Ok(())
    })
}
