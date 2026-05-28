//! `knogg hooks` — deterministic event hooks.
//!
//! Hooks run knogg-internal actions at fixed lifecycle events to keep the
//! brief and generated configs fresh without agents asking for it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::vault::resolve_path;
use crate::core::vaultio::{atomic_write, VaultLock};

/// Lifecycle events that may carry hooks.
pub const KNOWN_EVENTS: [&str; 4] = [
    "before_handoff",
    "after_state_change",
    "after_proposal_apply",
    "before_mcp_response",
];

/// Actions a hook may run.
const KNOWN_ACTIONS: [&str; 3] = ["refresh_brief", "sync", "ensure_brief_fresh"];

#[derive(Debug, Default, Deserialize, Serialize)]
struct HookSet {
    #[serde(default)]
    hooks: BTreeMap<String, Hook>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Hook {
    enabled: bool,
    #[serde(default)]
    actions: Vec<String>,
}

fn hooks_path(root: &Path) -> PathBuf {
    root.join("plans/hooks.yml")
}

/// Load `plans/hooks.yml`; a missing file yields an empty set.
fn load(root: &Path) -> Result<HookSet> {
    match fs::read_to_string(hooks_path(root)) {
        Ok(raw) => serde_yaml::from_str(&raw)
            .map_err(|e| anyhow!("parsing hooks.yml: {e}")),
        Err(_) => Ok(HookSet::default()),
    }
}

/// Atomically write the hook set. Caller must hold the lock.
fn write(root: &Path, set: &HookSet) -> Result<()> {
    let out = serde_yaml::to_string(set).map_err(|e| anyhow!("serializing hooks.yml: {e}"))?;
    atomic_write(&hooks_path(root), out.as_bytes())
}

/// Run every action for `event` if the event is enabled. No-op if absent.
///
/// Deterministic: actions run in declared order; the first failure aborts
/// with a clear error naming the event and action.
pub fn run(root: &Path, event: &str) -> Result<()> {
    let set = load(root)?;
    let Some(hook) = set.hooks.get(event) else {
        return Ok(());
    };
    if !hook.enabled {
        return Ok(());
    }
    for action in &hook.actions {
        run_action(root, action)
            .with_context(|| format!("hook '{event}' action '{action}'"))?;
    }
    Ok(())
}

fn run_action(root: &Path, action: &str) -> Result<()> {
    match action {
        "refresh_brief" => {
            crate::commands::brief::refresh(root)?;
            Ok(())
        }
        "ensure_brief_fresh" => crate::commands::brief::ensure_fresh(root),
        // "sync" kept as known action for backward compat with existing hooks.yml files;
        // now refreshes the brief instead of rendering tool configs.
        "sync" => crate::commands::brief::refresh(root).map(|_| ()),
        other => bail!("unknown hook action '{other}'"),
    }
}

// ---- CLI -------------------------------------------------------------------

pub fn cmd_list(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let set = load(&root)?;
    if set.hooks.is_empty() {
        println!("no hooks");
        return Ok(());
    }
    for (event, hook) in &set.hooks {
        let state = if hook.enabled { "enabled " } else { "disabled" };
        println!("{event:22} {state}  actions: {}", hook.actions.join(", "));
    }
    Ok(())
}

pub fn cmd_doctor(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let set = load(&root)?;
    println!("hooks doctor\n");
    let mut errors = 0u32;

    for (event, hook) in &set.hooks {
        if KNOWN_EVENTS.contains(&event.as_str()) {
            println!("[ok] event {event}");
        } else {
            println!("[error] unknown event '{event}'");
            errors += 1;
        }
        for action in &hook.actions {
            if KNOWN_ACTIONS.contains(&action.as_str()) {
                println!("[ok] {event} -> {action}");
            } else {
                println!("[error] {event}: unknown action '{action}'");
                errors += 1;
            }
        }
    }

    println!();
    if errors > 0 {
        println!("Result: unhealthy");
        std::process::exit(1);
    }
    println!("Result: healthy");
    Ok(())
}

pub fn cmd_run(path: &str, event: &str) -> Result<()> {
    if !KNOWN_EVENTS.contains(&event) {
        bail!("unknown event '{event}'");
    }
    let root = resolve_path(path)?;
    run(&root, event)?;
    println!("hook {event} done");
    Ok(())
}

pub fn cmd_set_enabled(path: &str, event: &str, enabled: bool) -> Result<()> {
    if !KNOWN_EVENTS.contains(&event) {
        bail!("unknown event '{event}'");
    }
    let root = resolve_path(path)?;
    let _lock = VaultLock::acquire(&root)?;
    let mut set = load(&root)?;
    set.hooks
        .entry(event.to_string())
        .or_insert(Hook { enabled, actions: Vec::new() })
        .enabled = enabled;
    write(&root, &set)?;
    println!("hook {event} {}", if enabled { "enabled" } else { "disabled" });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::init;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("knogg-hooks-{label}-{nanos}"))
    }

    #[test]
    fn default_hooks_parse_and_run() {
        let set: HookSet = serde_yaml::from_str(crate::core::vault::DEFAULT_HOOKS).unwrap();
        for e in KNOWN_EVENTS {
            assert!(set.hooks.contains_key(e), "missing event {e}");
        }
        for a in set.hooks.values().flat_map(|h| &h.actions) {
            assert!(KNOWN_ACTIONS.contains(&a.as_str()), "unknown action {a}");
        }
    }

    #[test]
    fn run_event_executes_actions() {
        let root = temp_root("run");
        init(root.to_str().unwrap(), false).unwrap();
        // before_handoff -> refresh_brief: produces state/brief.yml.
        run(&root, "before_handoff").unwrap();
        assert!(root.join("state/brief.yml").is_file());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn disabled_event_is_a_noop() {
        let root = temp_root("disabled");
        init(root.to_str().unwrap(), false).unwrap();
        cmd_set_enabled(root.to_str().unwrap(), "before_handoff", false).unwrap();
        run(&root, "before_handoff").unwrap();
        assert!(!root.join("state/brief.yml").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unknown_action_is_rejected() {
        assert!(run_action(Path::new("/tmp/x"), "bogus").is_err());
    }
}
