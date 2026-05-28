use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub fn link(ide: &str) -> Result<()> {
    let bin = std::env::current_exe()?;
    match ide {
        "cursor" => link_cursor(&bin),
        "claude" => link_claude(&bin),
        other => Err(anyhow!("unknown ide '{other}' — supported: cursor, claude")),
    }
}

fn mcp_entry(bin: &Path) -> Value {
    json!({
        "command": bin.to_string_lossy(),
        "args": ["mcp", "--path", ".knogg"]
    })
}

fn write_mcp_json(config_path: &PathBuf, key: &str, entry: Value) -> Result<()> {
    let mut cfg: Value = if config_path.exists() {
        let raw = std::fs::read_to_string(config_path)?;
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if cfg.get(key).is_none() {
        cfg[key] = json!({});
    }
    cfg[key]["knogg"] = entry;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, serde_json::to_string_pretty(&cfg)?)?;
    Ok(())
}

fn link_cursor(bin: &Path) -> Result<()> {
    let config_path = PathBuf::from(".cursor/mcp.json");
    write_mcp_json(&config_path, "mcpServers", mcp_entry(bin))?;
    println!("linked: {} → {}", bin.display(), config_path.display());
    Ok(())
}

fn link_claude(bin: &Path) -> Result<()> {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| anyhow!("$HOME is not set"))?;
    let config_path = home.join(".claude.json");
    write_mcp_json(&config_path, "mcpServers", mcp_entry(bin))?;
    println!("linked: {} → {}", bin.display(), config_path.display());
    Ok(())
}
