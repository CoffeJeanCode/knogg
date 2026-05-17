//! Agent-to-agent message log at `state/messages.yml`.
//!
//! Lightweight channel: an agent posts a note, others read recent notes.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::vaultio::{atomic_write, today, VaultLock};

#[derive(Debug, Default, Deserialize, Serialize)]
struct MessageLog {
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Message {
    id: String,
    from: String,
    at: String,
    text: String,
}

fn log_path(root: &Path) -> std::path::PathBuf {
    root.join("state/messages.yml")
}

fn load(root: &Path) -> Result<MessageLog> {
    match fs::read_to_string(log_path(root)) {
        Ok(raw) => serde_yaml::from_str(&raw)
            .map_err(|e| anyhow!("parsing messages.yml: {e}")),
        Err(_) => Ok(MessageLog::default()),
    }
}

/// Post a message; returns its id (`MSG-NNNN`).
pub fn post(root: &Path, from: &str, text: &str) -> Result<String> {
    let _lock = VaultLock::acquire(root)?;
    let mut log = load(root)?;
    let id = format!("MSG-{:04}", log.messages.len() + 1);
    log.messages.push(Message {
        id: id.clone(),
        from: from.to_string(),
        at: today(),
        text: text.to_string(),
    });
    let out = serde_yaml::to_string(&log)
        .map_err(|e| anyhow!("serializing messages.yml: {e}"))?;
    atomic_write(&log_path(root), out.as_bytes())?;
    Ok(id)
}

/// Recent messages as JSON (newest last). `limit` caps the count.
pub fn recent_json(root: &Path, limit: Option<usize>) -> Result<Value> {
    let log = load(root)?;
    let msgs = &log.messages;
    let slice = match limit {
        Some(n) if n < msgs.len() => &msgs[msgs.len() - n..],
        _ => &msgs[..],
    };
    Ok(json!({ "messages": serde_json::to_value(slice)? }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::init;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("knogg-msg-{label}-{nanos}"))
    }

    #[test]
    fn post_then_read_messages() {
        let root = temp_root("post");
        init(root.to_str().unwrap(), false).unwrap();

        assert_eq!(post(&root, "cursor", "starting auth work").unwrap(), "MSG-0001");
        post(&root, "claude", "reviewed, looks good").unwrap();

        let all = recent_json(&root, None).unwrap();
        assert_eq!(all["messages"].as_array().unwrap().len(), 2);

        let last = recent_json(&root, Some(1)).unwrap();
        let m = &last["messages"].as_array().unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0]["from"], "claude");
        fs::remove_dir_all(&root).ok();
    }
}
