//! P2P serve module.
//!
//! `knogg serve --port <PORT>` runs a TCP JSON-RPC server exposing the
//! read-only MCP surface ([`crate::mcp::READ_ONLY_TOOLS`]).
//! Also accepts `subscribe_to_task` registrations and pushes
//! `task_done` events back over the same socket.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

pub use crate::mcp::dispatch_line_readonly;

pub struct ServeConfig {
    pub vault_root: PathBuf,
    pub port: u16,
}

pub async fn serve(config: ServeConfig) -> Result<()> {
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("[mesh:serve] listening on {} (read-only)", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let root = config.vault_root.clone();
                tokio::spawn(handle_connection(stream, peer, root));
            }
            Err(e) => eprintln!("[mesh:serve] accept error: {}", e),
        }
    }
}

async fn handle_connection(stream: TcpStream, peer: SocketAddr, root: PathBuf) {
    let (rd, wr) = stream.into_split();
    let mut lines = BufReader::new(rd).lines();
    let writer = Arc::new(Mutex::new(wr));

    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Pre-parse to catch subscribe_to_task (needs the live socket).
        if let Ok(req) = serde_json::from_str::<Value>(trimmed) {
            let method = req.get("method").and_then(Value::as_str).unwrap_or("");
            let inner = if method == "tools/call" {
                req.get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
            } else {
                method
            };
            if inner == "subscribe_to_task" {
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                let args = if method == "tools/call" {
                    req.get("params")
                        .and_then(|p| p.get("arguments"))
                        .cloned()
                        .unwrap_or(json!({}))
                } else {
                    req.get("params").cloned().unwrap_or(json!({}))
                };
                let task_id = args
                    .get("task_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if task_id.is_empty() {
                    let resp = json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": {"code": -32602, "message": "missing task_id"}
                    });
                    let _ = send_line(&writer, &resp.to_string()).await;
                    continue;
                }
                crate::mesh::events::register_remote_subscriber(
                    &task_id,
                    &peer.to_string(),
                    writer.clone(),
                );
                let resp = json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"subscribed": true, "task_id": task_id}
                });
                let _ = send_line(&writer, &resp.to_string()).await;
                continue;
            }
        }

        if let Some(response) = dispatch_line_readonly(&root, trimmed) {
            if send_line(&writer, &response).await.is_err() {
                break;
            }
        }
    }
    eprintln!("[mesh:serve] client {} disconnected", peer);
}

async fn send_line(
    writer: &Arc<Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    msg: &str,
) -> std::io::Result<()> {
    let mut w = writer.lock().await;
    w.write_all(msg.as_bytes()).await?;
    w.write_all(b"\n").await?;
    w.flush().await
}
