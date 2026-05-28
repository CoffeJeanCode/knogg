//! Agent-to-agent message log at `state/messages.yml`.
//!
//! Structured messages: optional `to`, `reply_to`, `read_by`, and `status`.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::vaultio::{atomic_write, today, VaultLock};

pub const STATUS_OPEN: &str = "open";
pub const STATUS_ACKED: &str = "acked";
pub const STATUS_CLOSED: &str = "closed";

#[derive(Debug, Default, Deserialize, Serialize)]
struct MessageLog {
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub at: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_by: Vec<String>,
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_status() -> String {
    STATUS_OPEN.to_string()
}

#[derive(Debug, Default, Clone)]
pub struct MessageFilter {
    pub from: Option<String>,
    pub to: Option<String>,
    pub status: Option<String>,
    pub unread_for: Option<String>,
    pub limit: Option<usize>,
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

fn write_log(root: &Path, log: &MessageLog) -> Result<()> {
    let out = serde_yaml::to_string(log).map_err(|e| anyhow!("serializing messages.yml: {e}"))?;
    atomic_write(&log_path(root), out.as_bytes())
}

fn validate_status(status: &str) -> Result<()> {
    if matches!(status, STATUS_OPEN | STATUS_ACKED | STATUS_CLOSED) {
        Ok(())
    } else {
        bail!("invalid message status '{status}' (allowed: open, acked, closed)")
    }
}

fn matches_filter(m: &Message, f: &MessageFilter) -> bool {
    if let Some(from) = &f.from {
        if m.from != *from {
            return false;
        }
    }
    if let Some(to) = &f.to {
        if !m.to.is_empty() && !m.to.iter().any(|t| t == to) {
            return false;
        }
    }
    if let Some(status) = &f.status {
        if m.status != *status {
            return false;
        }
    }
    if let Some(agent) = &f.unread_for {
        if m.read_by.iter().any(|r| r == agent) {
            return false;
        }
        if !m.to.is_empty() && !m.to.iter().any(|t| t == agent) {
            return false;
        }
    }
    true
}

/// Post a message; returns its id (`MSG-NNNN`).
pub fn post(
    root: &Path,
    from: &str,
    text: &str,
    to: Option<Vec<String>>,
    reply_to: Option<String>,
) -> Result<String> {
    let _lock = VaultLock::acquire(root)?;
    let mut log = load(root)?;
    let id = format!("MSG-{:04}", log.messages.len() + 1);
    log.messages.push(Message {
        id: id.clone(),
        from: from.to_string(),
        at: today(),
        text: text.to_string(),
        to: to.unwrap_or_default(),
        reply_to,
        read_by: Vec::new(),
        status: STATUS_OPEN.to_string(),
    });
    write_log(root, &log)?;
    Ok(id)
}

fn ack_message(log: &mut MessageLog, id: &str, by: &str) -> Result<()> {
    let m = log
        .messages
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow!("message '{id}' not found"))?;
    if !m.read_by.iter().any(|r| r == by) {
        m.read_by.push(by.to_string());
    }
    if m.status == STATUS_OPEN {
        m.status = STATUS_ACKED.to_string();
    }
    Ok(())
}

/// Ack many messages under one lock. Best-effort — not atomic.
pub fn ack_many(root: &Path, ids: &[String], by: &str) -> Result<Vec<(String, Result<()>)>> {
    let _lock = VaultLock::acquire(root)?;
    let mut log = load(root)?;
    let results: Vec<_> = ids
        .iter()
        .map(|id| (id.clone(), ack_message(&mut log, id, by)))
        .collect();
    write_log(root, &log)?;
    Ok(results)
}

/// Close open messages older than `max_open_days` (ISO date on `at` field).
pub fn expire_stale(root: &Path, max_open_days: u32) -> Result<usize> {
    let cutoff = days_ago(max_open_days);
    let _lock = VaultLock::acquire(root)?;
    let mut log = load(root)?;
    let mut n = 0usize;
    for m in &mut log.messages {
        if m.status == STATUS_OPEN && m.at.as_str() < cutoff.as_str() {
            m.status = STATUS_CLOSED.to_string();
            n += 1;
        }
    }
    if n > 0 {
        write_log(root, &log)?;
    }
    Ok(n)
}

fn days_ago(days: u32) -> String {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        .saturating_sub(u64::from(days) * 86_400);
    let (y, m, d) = unix_day_to_ymd(t / 86_400);
    format!("{y:04}-{m:02}-{d:02}")
}

fn unix_day_to_ymd(mut days: u64) -> (u32, u32, u32) {
    let mut y = 1970u32;
    loop {
        let diy = if is_leap(y) { 366 } else { 365 };
        if days < diy {
            break;
        }
        days -= diy;
        y += 1;
    }
    let mut m = 1u32;
    for md in month_days(y) {
        if days < md {
            break;
        }
        days -= md;
        m += 1;
    }
    (y, m, (days + 1) as u32)
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_days(y: u32) -> [u64; 12] {
    [
        31,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ]
}

/// Terse message rows for MCP inbox (used by Resources and internally).
#[allow(dead_code)]
pub fn inbox_terse(root: &Path, agent: Option<&str>) -> Result<Value> {
    expire_stale(root, 30)?;
    let filter = MessageFilter {
        status: Some(STATUS_OPEN.to_string()),
        unread_for: agent.map(String::from),
        limit: Some(20),
        ..Default::default()
    };
    let log = load(root)?;
    let msgs: Vec<Value> = log
        .messages
        .iter()
        .filter(|m| matches_filter(m, &filter))
        .map(|m| {
            json!({
                "id": m.id,
                "from": m.from,
                "st": m.status,
                "tx": m.text,
            })
        })
        .collect();
    Ok(json!(msgs))
}

/// Filtered messages as JSON (`messages` array, newest last).
pub fn filtered_json(root: &Path, filter: &MessageFilter) -> Result<Value> {
    let _ = expire_stale(root, 30);
    let log = load(root)?;
    let mut out: Vec<&Message> = log.messages.iter().filter(|m| matches_filter(m, filter)).collect();
    if let Some(n) = filter.limit {
        if n < out.len() {
            out = out.split_off(out.len() - n);
        }
    }
    Ok(json!({ "messages": serde_json::to_value(out)? }))
}

// ---- CLI -------------------------------------------------------------------

pub fn cmd_list(path: &str, filter: MessageFilter) -> Result<()> {
    let root = crate::core::vault::resolve_path(path)?;
    if let Some(s) = &filter.status {
        validate_status(s)?;
    }
    let v = filtered_json(&root, &filter)?;
    for m in v["messages"].as_array().unwrap_or(&vec![]) {
        let id = m["id"].as_str().unwrap_or("?");
        let from = m["from"].as_str().unwrap_or("?");
        let status = m["status"].as_str().unwrap_or("?");
        println!("{id}  {from:10}  {status:6}  {}", m["text"].as_str().unwrap_or(""));
    }
    Ok(())
}

pub fn cmd_ack(path: &str, ids: &[String], by: &str) -> Result<()> {
    let root = crate::core::vault::resolve_path(path)?;
    let results = ack_many(&root, ids, by)?;
    let mut any_fail = false;
    for (id, res) in &results {
        match res {
            Ok(()) => println!("acked {id} by {by}"),
            Err(e) => {
                println!("FAILED {id}: {e:#}");
                any_fail = true;
            }
        }
    }
    if any_fail {
        bail!("one or more messages failed to ack");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::init;
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
    fn post_structured_and_ack() {
        let root = temp_root("structured");
        init(root.to_str().unwrap(), false).unwrap();

        let id = post(
            &root,
            "claude",
            "plan ready",
            Some(vec!["cursor".into()]),
            None,
        )
        .unwrap();
        assert_eq!(id, "MSG-0001");

        ack_many(&root, &[id.clone()], "cursor").unwrap();
        let v = filtered_json(
            &root,
            &MessageFilter {
                unread_for: Some("cursor".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(v["messages"].as_array().unwrap().is_empty());

        let read = filtered_json(&root, &MessageFilter::default()).unwrap();
        assert_eq!(read["messages"][0]["status"], STATUS_ACKED);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn filter_by_to_and_status() {
        let root = temp_root("filter");
        init(root.to_str().unwrap(), false).unwrap();
        post(&root, "a", "broadcast", None, None).unwrap();
        post(
            &root,
            "b",
            "for cursor",
            Some(vec!["cursor".into()]),
            None,
        )
        .unwrap();

        let v = filtered_json(
            &root,
            &MessageFilter {
                to: Some("cursor".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(v["messages"].as_array().unwrap().len(), 2);
        fs::remove_dir_all(&root).ok();
    }
}
