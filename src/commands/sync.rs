//! `knogg sync` — render tool_registry templates into agent config files.

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use minijinja::Environment;
use serde::Deserialize;

use crate::core::vault::{read_active_context, resolve_path};
use crate::core::vaultio::{atomic_write, backup_file, timestamp, VaultLock};

/// `plans/tool_registry.yml`: template -> output mappings.
#[derive(Debug, Deserialize)]
struct ToolRegistry {
    #[serde(default)]
    tools: Vec<ToolEntry>,
}

#[derive(Debug, Deserialize)]
struct ToolEntry {
    name: String,
    template: String,
    output: String,
}

/// What `sync` would do with one output file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Create,
    Update,
    SkipHuman,
    Unchanged,
}

/// `knogg sync`: render tool configs from the registry templates.
///
/// `marker` is the generated-by marker (from `knogg.toml`, or the default).
/// With `dry_run`, nothing is created/modified/deleted — only a plan is shown.
pub fn sync(path: &str, force: bool, marker: &str, dry_run: bool) -> Result<()> {
    let root = resolve_path(path)?;
    // Serialize against concurrent writers; dry-run touches nothing, no lock.
    let _lock = if dry_run {
        None
    } else {
        Some(VaultLock::acquire(&root)?)
    };

    let registry_path = root.join("plans/tool_registry.yml");
    let raw = fs::read_to_string(&registry_path)
        .with_context(|| format!("reading {} (run `knogg init`?)", registry_path.display()))?;
    let registry: ToolRegistry = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("parsing {}: {e}", registry_path.display()))?;

    // Inject only the active context, never the full vault.
    let ctx = read_active_context(&root)?;

    // One timestamp groups all backups produced by this run.
    let stamp = timestamp();
    for tool in &registry.tools {
        sync_tool(&root, tool, &ctx, force, marker, dry_run, &stamp)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sync_tool(
    root: &Path,
    tool: &ToolEntry,
    ctx: &crate::core::vault::ActiveContext,
    force: bool,
    marker: &str,
    dry_run: bool,
    stamp: &str,
) -> Result<()> {
    // Template lives inside the vault; reject traversal out of it.
    let template_rel = resolve_path(&tool.template)?;
    let template_path = root.join(&template_rel);
    let template = fs::read_to_string(&template_path).with_context(|| {
        format!(
            "reading template for '{}' at {}",
            tool.name,
            template_path.display()
        )
    })?;

    let mut env = Environment::new();
    env.add_template("t", &template)
        .map_err(|e| anyhow!("loading template for '{}': {e}", tool.name))?;
    let rendered = env
        .get_template("t")
        .unwrap()
        .render(ctx)
        .map_err(|e| anyhow!("rendering '{}': {e}", tool.name))?;

    // Ensure the generated-by marker is the first line of the output.
    let content = if rendered.starts_with(marker) {
        rendered
    } else {
        format!("{marker}\n{rendered}")
    };

    // Output is project-relative; reject traversal escapes.
    let output_path = resolve_path(&tool.output)?;
    let display = output_path.display().to_string();

    // Decide the action; this logic is shared by real and dry-run mode.
    let existing: Option<String> = if output_path.exists() {
        Some(
            fs::read_to_string(&output_path)
                .with_context(|| format!("reading existing {display}"))?,
        )
    } else {
        None
    };
    let action = match &existing {
        None => Action::Create,
        Some(e) if !e.contains(marker) && !force => Action::SkipHuman,
        Some(e) if *e == content => Action::Unchanged,
        Some(_) => Action::Update,
    };

    if dry_run {
        match action {
            Action::Create => println!("would create {display}"),
            Action::Update => println!("would update {display}"),
            Action::SkipHuman => println!("would skip {display} human-owned"),
            Action::Unchanged => println!("unchanged {display}"),
        }
        return Ok(());
    }

    match action {
        Action::SkipHuman => {
            println!("skip {display}: human-owned file (no marker); use --force to overwrite");
        }
        Action::Unchanged => {
            println!("unchanged {display}");
        }
        Action::Create | Action::Update => {
            // Under --force, back up the file being overwritten.
            if action == Action::Update && force {
                if let Some(old) = &existing {
                    backup_file(root, Path::new(&tool.output), old.as_bytes(), stamp)?;
                }
            }
            atomic_write(&output_path, content.as_bytes())?;
            println!("wrote {display}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::{init, MARKER};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vault-sync-{label}-{nanos}"))
    }

    /// Run a closure with the process CWD set to `dir`, restoring it after.
    fn with_cwd<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
        // Tests touching CWD must not run concurrently with each other.
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        let out = f();
        std::env::set_current_dir(prev).unwrap();
        out
    }

    #[test]
    fn sync_generates_and_is_idempotent() {
        let root = temp_root("idem");
        fs::create_dir_all(&root).unwrap();
        init(root.join(".knogg").to_str().unwrap(), false).unwrap();

        with_cwd(&root, || {
            sync("./.knogg", false, MARKER, false).unwrap();
            assert!(root.join(".cursorrules").is_file());
            assert!(root.join(".claude/context.md").is_file());
            assert!(root.join("AGENTS.md").is_file());

            let before = fs::read_to_string(root.join(".cursorrules")).unwrap();
            assert!(before.starts_with(MARKER));
            // Second run must not change the file.
            sync("./.knogg", false, MARKER, false).unwrap();
            let after = fs::read_to_string(root.join(".cursorrules")).unwrap();
            assert_eq!(before, after);
        });
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dry_run_creates_nothing() {
        let root = temp_root("dryrun");
        fs::create_dir_all(&root).unwrap();
        init(root.join(".knogg").to_str().unwrap(), false).unwrap();

        with_cwd(&root, || {
            // Dry-run before any output exists: writes nothing.
            sync("./.knogg", false, MARKER, true).unwrap();
            assert!(!root.join(".cursorrules").exists());
            assert!(!root.join(".knogg/.lock").exists());

            // Real sync, then dry-run again: still no changes.
            sync("./.knogg", false, MARKER, false).unwrap();
            let before = fs::read_to_string(root.join(".cursorrules")).unwrap();
            sync("./.knogg", false, MARKER, true).unwrap();
            assert_eq!(fs::read_to_string(root.join(".cursorrules")).unwrap(), before);
        });
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sync_respects_human_files() {
        let root = temp_root("human");
        fs::create_dir_all(&root).unwrap();
        init(root.join(".knogg").to_str().unwrap(), false).unwrap();
        let human = root.join(".cursorrules");
        fs::write(&human, "hand written rules\n").unwrap();

        with_cwd(&root, || {
            sync("./.knogg", false, MARKER, false).unwrap();
            // Human file lacks the marker -> untouched.
            assert_eq!(
                fs::read_to_string(&human).unwrap(),
                "hand written rules\n"
            );
            // --force overwrites it.
            sync("./.knogg", true, MARKER, false).unwrap();
            assert!(fs::read_to_string(&human).unwrap().starts_with(MARKER));
        });
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn force_backs_up_overwritten_human_file() {
        let root = temp_root("forcebackup");
        fs::create_dir_all(&root).unwrap();
        init(root.join(".knogg").to_str().unwrap(), false).unwrap();
        fs::write(root.join(".cursorrules"), "hand written rules\n").unwrap();

        with_cwd(&root, || {
            sync("./.knogg", true, MARKER, false).unwrap();

            let backups = root.join(".knogg/backups");
            assert!(backups.is_dir(), "no backups directory created");
            let stamp_dir = fs::read_dir(&backups)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            assert_eq!(
                fs::read_to_string(stamp_dir.join(".cursorrules")).unwrap(),
                "hand written rules\n"
            );
        });
        fs::remove_dir_all(&root).ok();
    }
}
