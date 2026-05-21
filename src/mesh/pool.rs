//! Declarative peering — Stage 10.
//!
//! Static topology declared in `knogg.toml [mesh.peers]`. The pool spawns a
//! supervisor thread per peer that maintains a live TCP/JSON-RPC connection,
//! auto-reconnecting on failure with exponential backoff (1s..30s).

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;

static COUNTER: AtomicU64 = AtomicU64::new(0);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct PeerState {
    writer: Mutex<Option<Arc<Mutex<TcpStream>>>>,
    pending: Mutex<HashMap<String, mpsc::SyncSender<Value>>>,
    subscriptions: Mutex<HashMap<String, Box<dyn Fn(&Value) + Send + 'static>>>,
    stopping: AtomicBool,
}

pub struct PeerHandle {
    state: Arc<PeerState>,
    addr: String,
}

impl PeerHandle {
    pub fn addr(&self) -> &str { &self.addr }

    pub fn is_connected(&self) -> bool {
        self.state.writer.lock().unwrap().is_some()
    }

    pub fn query(&self, method: &str, params: &Value) -> Result<Value> {
        let writer = self.state.writer.lock().unwrap().clone()
            .ok_or_else(|| anyhow!("peer not connected"))?;

        let id = format!("p{}", COUNTER.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = mpsc::sync_channel::<Value>(1);
        self.state.pending.lock().unwrap().insert(id.clone(), tx);

        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        });
        write_line(&writer, &serde_json::to_string(&req)?)?;

        match rx.recv_timeout(QUERY_TIMEOUT) {
            Ok(v) => {
                if let Some(err) = v.get("error") {
                    Err(anyhow!("{}", err["message"].as_str().unwrap_or("remote error")))
                } else {
                    Ok(v.get("result").cloned().unwrap_or(v))
                }
            }
            Err(_) => {
                self.state.pending.lock().unwrap().remove(&id);
                Err(anyhow!("query to peer timed out"))
            }
        }
    }

    pub fn subscribe_task<F>(&self, task_id: &str, cb: F) -> Result<()>
    where F: Fn(&Value) + Send + 'static,
    {
        self.state.subscriptions.lock().unwrap()
            .insert(task_id.to_string(), Box::new(cb));
        let _ = self.query("subscribe_to_task",
            &serde_json::json!({"task_id": task_id}));
        Ok(())
    }
}

fn write_line(writer: &Arc<Mutex<TcpStream>>, line: &str) -> Result<()> {
    let mut w = writer.lock().unwrap();
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}

fn parse_addr(addr: &str) -> Result<std::net::SocketAddr> {
    let stripped = addr.strip_prefix("tcp://").unwrap_or(addr);
    stripped.to_socket_addrs().ok().and_then(|mut it| it.next())
        .ok_or_else(|| anyhow!("invalid peer address '{}'", addr))
}

fn supervise(name: String, addr: String, state: Arc<PeerState>) {
    let mut backoff = BACKOFF_MIN;
    loop {
        if state.stopping.load(Ordering::Relaxed) { return; }
        let sock = match parse_addr(&addr) {
            Ok(s) => s,
            Err(e) => { eprintln!("[mesh] peer '{}' bad addr: {}", name, e); return; }
        };
        match TcpStream::connect_timeout(&sock, Duration::from_secs(3)) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                let reader = match stream.try_clone() {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("[mesh] peer '{}' clone failed: {}", name, e);
                        thread::sleep(backoff); continue;
                    }
                };
                let writer = Arc::new(Mutex::new(stream));
                *state.writer.lock().unwrap() = Some(writer.clone());
                eprintln!("[mesh] peer '{}' connected", name);
                backoff = BACKOFF_MIN;

                let subs: Vec<String> = state.subscriptions.lock().unwrap()
                    .keys().cloned().collect();
                for task_id in subs {
                    let req = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": format!("p{}", COUNTER.fetch_add(1, Ordering::Relaxed)),
                        "method": "subscribe_to_task",
                        "params": {"task_id": task_id},
                    });
                    let _ = write_line(&writer, &req.to_string());
                }

                read_loop(reader, &state);
                *state.writer.lock().unwrap() = None;
                eprintln!("[mesh] peer '{}' disconnected", name);
            }
            Err(e) => {
                eprintln!("[mesh] peer '{}' connect failed: {} (retry in {:?})",
                    name, e, backoff);
            }
        }
        if state.stopping.load(Ordering::Relaxed) { return; }
        thread::sleep(backoff);
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

fn read_loop(reader: TcpStream, state: &Arc<PeerState>) {
    use std::io::{BufRead, BufReader};
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        let Ok(msg): Result<Value, _> = serde_json::from_str(&line) else { continue };

        if let Some(result) = msg.get("result") {
            if result.get("type").and_then(Value::as_str) == Some("task_done") {
                let task_id = result.get("task_id").and_then(Value::as_str).unwrap_or("");
                if let Some(cb) = state.subscriptions.lock().unwrap().get(task_id) {
                    cb(result);
                    continue;
                }
            }
        }

        let id = msg.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(tx) = state.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(msg);
        }
    }
}

pub struct Pool {
    peers: Mutex<HashMap<String, PeerHandle>>,
}

impl Pool {
    pub fn new() -> Self {
        Self { peers: Mutex::new(HashMap::new()) }
    }

    /// Register a peer. Non-blocking — spawns supervisor and returns immediately.
    pub fn add_peer(&self, name: &str, addr: &str) {
        let state = Arc::new(PeerState::default());
        let handle = PeerHandle { state: state.clone(), addr: addr.to_string() };
        self.peers.lock().unwrap().insert(name.to_string(), handle);

        let n = name.to_string();
        let a = addr.to_string();
        thread::spawn(move || supervise(n, a, state));
    }

    pub fn query(&self, peer: &str, method: &str, params: &Value) -> Result<Value> {
        let peers = self.peers.lock().unwrap();
        let h = peers.get(peer).ok_or_else(|| anyhow!("unknown peer '{}'", peer))?;
        h.query(method, params)
    }

    pub fn subscribe<F>(&self, peer: &str, task_id: &str, cb: F) -> Result<()>
    where F: Fn(&Value) + Send + 'static,
    {
        let peers = self.peers.lock().unwrap();
        let h = peers.get(peer).ok_or_else(|| anyhow!("unknown peer '{}'", peer))?;
        h.subscribe_task(task_id, cb)
    }

    pub fn peers(&self) -> Vec<String> {
        self.peers.lock().unwrap().keys().cloned().collect()
    }
}

impl Default for Pool {
    fn default() -> Self { Self::new() }
}
