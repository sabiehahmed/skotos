use std::io::{self, BufRead, Write};
use std::net::SocketAddr;

use skotos_mcp::{JsonRpcRequest, McpConfig, McpServer};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let server = McpServer::new(McpConfig::default());

    if let Some(pos) = args.iter().position(|a| a == "--http") {
        let addr_str = args
            .get(pos + 1)
            .cloned()
            .unwrap_or_else(|| "127.0.0.1:8084".to_string());
        let addr: SocketAddr = addr_str.parse()?;
        let listener = TcpListener::bind(addr).await?;
        eprintln!("skotos-mcp HTTP server listening on http://{addr}/mcp");
        axum::serve(listener, server.http_router()).await?;
        return Ok(());
    }

    // Standard MCP stdio newline-delimited JSON-RPC 2.0 loop
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(&line) {
            if let Some(resp) = server.handle_request(req).await {
                serde_json::to_writer(&mut stdout, &resp)?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
            }
        }
    }
    Ok(())
}
