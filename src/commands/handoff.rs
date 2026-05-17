use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use minijinja::Environment;
use serde_json::{json, Value};

use crate::core::vault::resolve_path;
use crate::core::vaultio::{atomic_write, VaultLock};

/// Map a CLI agent name to its adapter file (relative to the vault root).
fn adapter_for(agent: &str) -> Result<&'static str> {
    match agent {
        "cursor" => Ok("adapters/cursor_prompt.md"),
        "claude" => Ok("adapters/claude_code.md"),
        "codex" => Ok("adapters/codex_prompt.md"),
        other => bail!("unknown agent '{other}' (expected: cursor, claude, codex)"),
    }
}

/// Render an agent handoff prompt from the compact brief (never the full vault).
///
/// Honors the agent's context profile from `agent_registry.yml` if present.
pub fn render(root: &Path, agent: &str) -> Result<String> {
    let adapter_rel = adapter_for(agent)?;
    let adapter_path = root.join(adapter_rel);
    let template = fs::read_to_string(&adapter_path).with_context(|| {
        format!(
            "reading adapter for '{agent}' at {} (run `knogg init`?)",
            adapter_path.display()
        )
    })?;

    // Read the compact brief, not the whole vault.
    let brief = crate::commands::brief::load_or_refresh(root)?;
    let profile = crate::commands::agents::agent_profile(root, agent);
    let ctx = brief_context(&brief, profile.as_ref());

    let mut env = Environment::new();
    env.add_template("handoff", &template)
        .map_err(|e| anyhow!("loading adapter template: {e}"))?;
    let tmpl = env.get_template("handoff").unwrap();
    tmpl.render(&ctx)
        .map_err(|e| anyhow!("rendering handoff prompt: {e}"))
}

/// Build the minijinja context from the brief, trimmed by the agent profile.
fn brief_context(brief: &crate::commands::brief::Brief, profile: Option<&crate::commands::agents::AgentProfile>) -> Value {
    let mut next_actions = brief.next_actions.clone();
    let mut decisions = brief.recent_decisions.clone();
    if let Some(p) = profile {
        if let Some(n) = p.max_next_actions {
            next_actions.truncate(n);
        }
        if let Some(n) = p.max_decisions {
            decisions.truncate(n);
        }
    }
    json!({
        "project": {"name": brief.project},
        "focus": brief.focus,
        "constraints": brief.constraints,
        "next_actions": next_actions,
        "handoff": {"summary": brief.handoff_summary},
        "decisions": decisions,
    })
}

/// `knogg handoff --to <agent>`: render a compact handoff prompt.
///
/// Output: `--save` writes to a file, `--print` writes to stdout (both may be
/// combined). With neither, fall back to clipboard-or-stdout.
pub fn handoff(agent: &str, path: &str, print: bool, save: Option<&str>) -> Result<()> {
    let root = resolve_path(path)?;
    // F6: before_handoff hooks (e.g. refresh the brief).
    if let Err(e) = crate::commands::hooks::run(&root, "before_handoff") {
        eprintln!("hook warning: {e}");
    }
    let rendered = render(&root, agent)?;

    let mut handled = false;

    if let Some(out) = save {
        // Hold the vault lock and write atomically; parent dirs are created.
        let _lock = VaultLock::acquire(&root)?;
        atomic_write(Path::new(out), rendered.as_bytes())
            .with_context(|| format!("saving handoff prompt to {out}"))?;
        println!("Handoff prompt saved to {out}");
        handled = true;
    }
    if print {
        println!("{rendered}");
        handled = true;
    }
    // No explicit output requested: keep the original clipboard/stdout behavior.
    if !handled {
        emit(&rendered);
    }
    Ok(())
}

/// Copy the prompt to the clipboard if the feature is enabled, else print it.
#[cfg(feature = "clipboard")]
fn emit(rendered: &str) {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(rendered.to_string())) {
        Ok(()) => println!("Handoff prompt copied to clipboard."),
        Err(e) => {
            eprintln!("clipboard unavailable ({e}); printing instead:");
            println!("{rendered}");
        }
    }
}

#[cfg(not(feature = "clipboard"))]
fn emit(rendered: &str) {
    println!("{rendered}");
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
        std::env::temp_dir().join(format!("vault-handoff-{label}-{nanos}"))
    }

    #[test]
    fn unknown_agent_is_rejected() {
        assert!(adapter_for("netscape").is_err());
        assert!(adapter_for("cursor").is_ok());
        assert!(adapter_for("claude").is_ok());
        assert!(adapter_for("codex").is_ok());
    }

    #[test]
    fn handoff_renders_for_known_agent() {
        let root = temp_root("render");
        init(root.to_str().unwrap(), false).unwrap();

        handoff("cursor", root.to_str().unwrap(), true, None).unwrap();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn handoff_fails_when_agent_missing() {
        let root = temp_root("missing");
        init(root.to_str().unwrap(), false).unwrap();

        assert!(handoff("codex", root.to_str().unwrap(), true, None).is_ok());
        std::fs::remove_file(root.join("adapters/codex_prompt.md")).unwrap();
        assert!(handoff("codex", root.to_str().unwrap(), true, None).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn handoff_save_writes_file_and_creates_parents() {
        let root = temp_root("save");
        init(root.to_str().unwrap(), false).unwrap();

        let out = root.join("nested/dir/cursor.md");
        handoff(
            "cursor",
            root.to_str().unwrap(),
            false,
            Some(out.to_str().unwrap()),
        )
        .unwrap();

        let content = fs::read_to_string(&out).unwrap();
        assert!(content.contains("Handoff"));

        // --save overwrites an existing file.
        handoff(
            "claude",
            root.to_str().unwrap(),
            false,
            Some(out.to_str().unwrap()),
        )
        .unwrap();
        assert!(fs::read_to_string(&out).unwrap().contains("Claude"));

        std::fs::remove_dir_all(&root).ok();
    }
}
