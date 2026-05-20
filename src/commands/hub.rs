use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};

type Registry = Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>;

pub fn serve(port: u16) -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(serve_async(port))
}

async fn serve_async(port: u16) -> Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    println!("knogg hub listening on 0.0.0.0:{port}");
    let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
    loop {
        let (stream, addr) = listener.accept().await?;
        eprintln!("hub: connection from {addr}");
        tokio::spawn(handle_conn(stream, registry.clone()));
    }
}

async fn handle_conn(stream: TcpStream, registry: Registry) {
    let (reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    let mut lines = BufReader::new(reader).lines();

    let (tx, mut rx) = mpsc::channel::<String>(64);

    // Outbound write task
    let w = writer.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let mut w = w.lock().await;
            if w.write_all(msg.as_bytes()).await.is_err() {
                break;
            }
            if w.write_all(b"\n").await.is_err() {
                break;
            }
            let _ = w.flush().await;
        }
    });

    let mut project: Option<String> = None;

    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };
        match msg["type"].as_str().unwrap_or("") {
            "register" => {
                let name = msg["project"].as_str().unwrap_or("unknown").to_string();
                project = Some(name.clone());
                registry.lock().await.insert(name.clone(), tx.clone());
                let _ = tx
                    .send(json!({"type": "registered", "project": name}).to_string())
                    .await;
                eprintln!("hub: + {name}");
            }
            "query" => {
                let target = msg["target"].as_str().unwrap_or("").to_string();
                let id = msg["id"].as_str().unwrap_or("").to_string();
                let reg = registry.lock().await;
                if let Some(t) = reg.get(&target) {
                    let _ = t.send(line.clone()).await;
                } else {
                    drop(reg);
                    let _ = tx
                        .send(
                            json!({"type": "error", "id": id,
                                   "message": format!("'{target}' not connected")})
                            .to_string(),
                        )
                        .await;
                }
            }
            "response" | "error" => {
                let to = msg["to"].as_str().unwrap_or("").to_string();
                let reg = registry.lock().await;
                if let Some(t) = reg.get(&to) {
                    let _ = t.send(line.clone()).await;
                }
            }
            "list" => {
                let peers: Vec<String> = registry.lock().await.keys().cloned().collect();
                let _ = tx
                    .send(json!({"type": "peers", "peers": peers}).to_string())
                    .await;
            }
            _ => {}
        }
    }

    if let Some(name) = project {
        registry.lock().await.remove(&name);
        eprintln!("hub: - {name}");
    }
}
