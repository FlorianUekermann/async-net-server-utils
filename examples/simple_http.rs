use async_net_server_utils::tcp::TcpIncoming;
use futures::executor::block_on;
use futures::prelude::*;
use std::net::Ipv4Addr;

fn main() -> anyhow::Result<()> {
    block_on(async {
        let mut incoming = TcpIncoming::bind((Ipv4Addr::UNSPECIFIED, 8080))?.http();
        while let Some(request) = incoming.next().await {
            dbg!(request);
        }

        Ok(())
    })
}
