use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

type PendingMap = Arc<Mutex<HashMap<String, std::sync::mpsc::SyncSender<Value>>>>;

static QUERY_COUNTER: AtomicU64 = AtomicU64::new(0);
static CLIENT: OnceLock<Option<MeshClient>> = OnceLock::new();

pub struct MeshClient {
    writer: Arc<Mutex<TcpStream>>,
    pending: PendingMap,
    project: String,
}

impl MeshClient {
    pub fn connect(hub_url: &str, project: &str, vault_root: PathBuf) -> Result<Self> {
        let addr = hub_url.strip_prefix("tcp://").unwrap_or(hub_url);
        let stream = TcpStream::connect_timeout(
            &addr.parse()?,
            Duration::from_secs(3),
        )?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        let reader_stream = stream.try_clone()?;
        let writer = Arc::new(Mutex::new(stream));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

        write_line(&writer, &json!({"type": "register", "project": project}))?;

        let writer_r = writer.clone();
        let pending_r = pending.clone();
        let project_r = project.to_string();
        let vault_root_r = vault_root.clone();
        std::thread::spawn(move || {
            reader_loop(reader_stream, writer_r, pending_r, project_r, vault_root_r);
        });

        Ok(Self {
            writer,
            pending,
            project: project.to_string(),
        })
    }

    pub fn query(&self, target: &str, query: &str, args: &Value) -> Result<Value> {
        let id = format!("q{}", QUERY_COUNTER.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = std::sync::mpsc::sync_channel::<Value>(1);
        self.pending.lock().unwrap().insert(id.clone(), tx);

        write_line(
            &self.writer,
            &json!({
                "type": "query",
                "id": id,
                "from": self.project,
                "target": target,
                "query": query,
                "args": args,
            }),
        )?;

        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| {
                self.pending.lock().unwrap().remove(&id);
                anyhow!("query to '{target}' timed out")
            })
            .and_then(|v| {
                if v["type"] == "error" {
                    Err(anyhow!("{}", v["message"].as_str().unwrap_or("remote error")))
                } else {
                    Ok(v["result"].clone())
                }
            })
    }

    #[allow(dead_code)]
    pub fn list_peers(&self) -> Result<Vec<String>> {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Value>(1);
        self.pending
            .lock()
            .unwrap()
            .insert("__list__".to_string(), tx);
        write_line(&self.writer, &json!({"type": "list"}))?;
        let resp = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| anyhow!("list peers timed out"))?;
        Ok(resp["peers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }
}


/// Initialize the global client from env vars. No-op if already initialized or KNOGG_HUB_URL unset.
pub fn init_from_env(vault_root: PathBuf) {
    CLIENT.get_or_init(|| {
        let url = std::env::var("KNOGG_HUB_URL").ok()?;
        let project = std::env::var("KNOGG_PROJECT").unwrap_or_else(|_| {
            vault_root
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "knogg".to_string())
        });
        match MeshClient::connect(&url, &project, vault_root) {
            Ok(c) => {
                eprintln!("mesh: registered as '{project}' on {url}");
                Some(c)
            }
            Err(e) => {
                eprintln!("mesh: could not connect to hub at {url}: {e}");
                None
            }
        }
    });
}

pub fn with_client<F, T>(f: F) -> Option<T>
where
    F: FnOnce(&MeshClient) -> T,
{
    CLIENT.get()?.as_ref().map(f)
}

fn write_line(writer: &Arc<Mutex<TcpStream>>, msg: &Value) -> Result<()> {
    let mut s = writer.lock().unwrap();
    let line = serde_json::to_string(msg)?;
    s.write_all(line.as_bytes())?;
    s.write_all(b"\n")?;
    s.flush()?;
    Ok(())
}

fn reader_loop(
    stream: TcpStream,
    writer: Arc<Mutex<TcpStream>>,
    pending: PendingMap,
    project: String,
    vault_root: PathBuf,
) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(msg): Result<Value, _> = serde_json::from_str(&line) else { continue };

        match msg["type"].as_str().unwrap_or("") {
            "response" | "error" => {
                let id = msg["id"].as_str().unwrap_or("").to_string();
                if let Some(tx) = pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(msg);
                }
            }
            "peers" => {
                if let Some(tx) = pending.lock().unwrap().remove("__list__") {
                    let _ = tx.send(msg);
                }
            }
            "query" => {
                let id = msg["id"].as_str().unwrap_or("").to_string();
                let from = msg["from"].as_str().unwrap_or("").to_string();
                let query_type = msg["query"].as_str().unwrap_or("").to_string();
                let args = msg.get("args").cloned().unwrap_or(json!({}));

                let resp = match dispatch_query(&vault_root, &query_type, &args) {
                    Ok(result) => json!({
                        "type": "response",
                        "id": id,
                        "to": from,
                        "result": result,
                    }),
                    Err(e) => json!({
                        "type": "error",
                        "id": id,
                        "to": from,
                        "message": e.to_string(),
                    }),
                };
                let _ = write_line(&writer, &resp);
            }
            _ => {}
        }
    }
    eprintln!("mesh: disconnected from hub (project '{project}')");
}

fn dispatch_query(root: &Path, query: &str, args: &Value) -> Result<Value> {
    match query {
        "get_active_context" | "read_vault" | "list_vault" | "search_vault" => {
            crate::mcp::call_tool_pub(root, query, args)
        }
        other => Err(anyhow!("unsupported mesh query '{other}'")),
    }
}
