use anyhow::Result;
use rd_net::identity::DeviceIdentity;
use rd_net::iroh_transport::{parse_connection_ticket, RD_ALPN};
use rd_net::IrohTransport;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("debug,iroh=info,quinn=warn,noq=warn")
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        println!("Usage:");
        println!("  rd-connect-test listen          # Show your ticket and wait for connections");
        println!("  rd-connect-test connect <TICKET> # Connect to a remote machine");
        return Ok(());
    }

    let identity = DeviceIdentity::load_or_create(None)?;

    match args[1].as_str() {
        "listen" => {
            println!("Starting iroh endpoint...");
            let iroh = IrohTransport::new(identity).await?;
            let ticket = iroh.connection_ticket();
            let addr = iroh.endpoint_addr();

            println!("\n=== LISTENING ===");
            println!("Connection ticket: {ticket}");
            println!("\nEndpoint addr details: {addr:?}");
            println!("\nWaiting for incoming connection...");

            match tokio::time::timeout(Duration::from_secs(120), iroh.accept()).await {
                Ok(Ok(conn)) => {
                    println!("Connected! Remote: {:?}", conn.remote_id());
                    let (send, mut recv) = conn.accept_bi().await?;
                    println!("Got bi-stream. Waiting for data...");
                    let mut buf = vec![0u8; 1024];
                    let n = recv.read(&mut buf).await?.unwrap_or(0);
                    println!("Received {} bytes: {:?}", n, String::from_utf8_lossy(&buf[..n]));
                }
                Ok(Err(e)) => println!("Accept error: {e}"),
                Err(_) => println!("Timeout after 120s"),
            }
        }
        "connect" => {
            if args.len() < 3 {
                println!("Usage: rd-connect-test connect <TICKET>");
                return Ok(());
            }

            let ticket = &args[2];

            println!("Parsing ticket...");
            let addr = parse_connection_ticket(ticket)?;
            println!("Parsed endpoint addr: {addr:?}");
            println!("Remote ID: {:?}", addr.id);
            println!("Addresses: {:?}", addr.addrs);

            println!("\nStarting iroh endpoint...");
            let iroh = IrohTransport::new(identity).await?;
            println!("My endpoint addr: {:?}", iroh.endpoint_addr());

            println!("\nConnecting (30s timeout)...");
            match iroh.connect_by_ticket(ticket).await {
                Ok(conn) => {
                    println!("CONNECTED! Remote: {:?}", conn.remote_id());
                    let (mut send, recv) = conn.open_bi().await?;
                    use tokio::io::AsyncWriteExt;
                    send.write_all(b"hello from connect test!").await?;
                    println!("Sent test data.");
                }
                Err(e) => {
                    println!("CONNECTION FAILED: {e}");
                    println!("\nPossible causes:");
                    println!("  - Firewall blocking UDP on port 7842+ on either side");
                    println!("  - The remote machine closed the app");
                    println!("  - iroh relay server unreachable");
                }
            }
        }
        _ => println!("Unknown command: {}", args[1]),
    }

    Ok(())
}
