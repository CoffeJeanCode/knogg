//! Cross-vault event bus — Stage 12 server side.
//!
//! When a remote peer sends `subscribe_to_task`, the serve loop registers the
//! connection's writer half here. On local task completion ([`emit_task_done`])
//! we push a JSON-RPC notification back to every registered subscriber.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::Mutex as AsyncMutex;

type SharedWriter = Arc<AsyncMutex<OwnedWriteHalf>>;

static BUS: OnceLock<Bus> = OnceLock::new();

fn bus() -> &'static Bus {
    BUS.get_or_init(Bus::new)
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDoneEvent {
    pub task_id: String,
    pub status: String,
    pub by: String,
}

struct Subscriber {
    peer: String,
    writer: SharedWriter,
}

pub struct Bus {
    by_task: Mutex<HashMap<String, Vec<Subscriber>>>,
}

impl Bus {
    fn new() -> Self {
        Self { by_task: Mutex::new(HashMap::new()) }
    }
}

pub fn register_remote_subscriber(task_id: &str, peer: &str, writer: SharedWriter) {
    bus().by_task.lock().unwrap()
        .entry(task_id.to_string())
        .or_default()
        .push(Subscriber { peer: peer.to_string(), writer });
    eprintln!("[mesh:events] {} subscribed to task: {}", peer, task_id);
}

pub fn emit_task_done(task_id: &str, by: &str) {
    let subs: Vec<Subscriber> = {
        let mut map = bus().by_task.lock().unwrap();
        let Some(list) = map.get_mut(task_id) else { return };
        std::mem::take(list)
    };

    let task_id = task_id.to_string();
    let by = by.to_string();

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(deliver(task_id, by, subs));
    } else {
        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Runtime::new() {
                rt.block_on(deliver(task_id, by, subs));
            }
        });
    }
}

async fn deliver(task_id: String, by: String, subs: Vec<Subscriber>) {
    let msg = json!({
        "jsonrpc": "2.0", "id": "evt",
        "result": {
            "type": "task_done",
            "task_id": &task_id,
            "status": "done",
            "by": by,
        }
    });
    let line = msg.to_string();

    let mut survivors = Vec::new();
    for sub in subs {
        let ok = {
            let mut w = sub.writer.lock().await;
            w.write_all(line.as_bytes()).await.is_ok()
                && w.write_all(b"\n").await.is_ok()
                && w.flush().await.is_ok()
        };
        if ok {
            survivors.push(sub);
        } else {
            eprintln!("[mesh:events] dropping dead subscriber: {}", sub.peer);
        }
    }
    if !survivors.is_empty() {
        bus().by_task
            .lock()
            .unwrap()
            .entry(task_id)
            .or_default()
            .extend(survivors);
    }
}
