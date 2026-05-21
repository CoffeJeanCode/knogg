//! `knogg state` — safe edits to `state/active_context.yml`.

use std::path::Path;

use anyhow::{bail, Result};

use crate::core::vault::ALLOWED_STATUS;
use crate::core::vault::{read_active_context, resolve_path, write_active_context};
use crate::core::vaultio::VaultLock;

/// F6: run `after_state_change` hooks (lock already released).
fn after_state_change(root: &Path) {
    if let Err(e) = crate::commands::hooks::run(root, "after_state_change") {
        eprintln!("hook warning: {e}");
    }
}

/// `knogg state set`: update stage/task/status fields.
pub fn set(
    path: &str,
    stage: Option<String>,
    task: Option<String>,
    status: Option<String>,
) -> Result<()> {
    if stage.is_none() && task.is_none() && status.is_none() {
        bail!("nothing to set: pass at least one of --stage, --task, --status");
    }
    if let Some(s) = &status {
        if !ALLOWED_STATUS.contains(&s.as_str()) {
            bail!(
                "invalid status '{s}' (allowed: {})",
                ALLOWED_STATUS.join(", ")
            );
        }
    }

    let root = resolve_path(path)?;
    {
        let _lock = VaultLock::acquire(&root)?;
        let mut ctx = read_active_context(&root)?;
        if let Some(s) = stage {
            ctx.focus.stage = s;
        }
        if let Some(t) = task {
            ctx.focus.task = t;
        }
        if let Some(s) = status {
            ctx.focus.status = s.clone();
            if s == "done" && !ctx.focus.task.is_empty() {
                crate::mesh::events::emit_task_done(&ctx.focus.task, "human");
            }
        }
        write_active_context(&root, &ctx)?;
    }
    println!("state updated");
    after_state_change(&root);
    Ok(())
}

/// `knogg state add-next`: append a next action.
pub fn add_next(path: &str, action: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let total;
    {
        let _lock = VaultLock::acquire(&root)?;
        let mut ctx = read_active_context(&root)?;
        ctx.next_actions.push(action.to_string());
        total = ctx.next_actions.len();
        write_active_context(&root, &ctx)?;
    }
    println!("next action added ({total} total)");
    after_state_change(&root);
    Ok(())
}

/// `knogg state clear-next`: remove all next actions.
pub fn clear_next(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    {
        let _lock = VaultLock::acquire(&root)?;
        let mut ctx = read_active_context(&root)?;
        ctx.next_actions.clear();
        write_active_context(&root, &ctx)?;
    }
    println!("next actions cleared");
    after_state_change(&root);
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
        std::env::temp_dir().join(format!("vault-state-{label}-{nanos}"))
    }

    #[test]
    fn set_updates_fields() {
        let root = temp_root("set");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();

        set(
            p,
            Some("frontend-ui".into()),
            Some("Implement badge".into()),
            Some("in_progress".into()),
        )
        .unwrap();

        let ctx = read_active_context(&root).unwrap();
        assert_eq!(ctx.focus.stage, "frontend-ui");
        assert_eq!(ctx.focus.task, "Implement badge");
        assert_eq!(ctx.focus.status, "in_progress");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn set_rejects_invalid_status() {
        let root = temp_root("badstatus");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();
        assert!(set(p, None, None, Some("wip".into())).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn set_requires_a_field() {
        let root = temp_root("empty");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();
        assert!(set(p, None, None, None).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn add_and_clear_next_actions() {
        let root = temp_root("next");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();

        add_next(p, "Update billing page").unwrap();
        add_next(p, "Write tests").unwrap();
        assert_eq!(read_active_context(&root).unwrap().next_actions.len(), 2);

        clear_next(p).unwrap();
        assert!(read_active_context(&root).unwrap().next_actions.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
