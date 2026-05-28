use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

/// MCP `resources/list` result.
pub fn list_resources() -> Value {
    json!({"resources": [
        {
            "uri": "knogg://core/architecture",
            "name": "Architecture Overview",
            "description": "High-level architecture of the project (core/architecture.yml)",
            "mimeType": "text/plain"
        },
        {
            "uri": "knogg://state/active_context",
            "name": "Active Context",
            "description": "Current task, stage, constraints and next actions (state/active_context.yml)",
            "mimeType": "text/plain"
        },
    ]})
}

/// MCP `resources/read` — returns the raw YAML text for a known knogg:// URI.
pub fn read_resource(root: &Path, uri: &str) -> Result<Value> {
    let vault_rel = match uri {
        "knogg://core/architecture" => "core/architecture.yml",
        "knogg://state/active_context" => "state/active_context.yml",
        other => bail!("unknown resource uri '{other}' (available: knogg://core/architecture, knogg://state/active_context)"),
    };

    let path = root.join(vault_rel);
    let text = fs::read_to_string(&path)
        .map_err(|e| anyhow!("reading {vault_rel}: {e}"))?;

    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "text/plain",
            "text": text,
        }]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::init;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("knogg-res-{label}-{nanos}"))
    }

    #[test]
    fn list_returns_two_resources() {
        let v = list_resources();
        let res = v["resources"].as_array().unwrap();
        assert_eq!(res.len(), 2);
        let uris: Vec<&str> = res.iter().map(|r| r["uri"].as_str().unwrap()).collect();
        assert!(uris.contains(&"knogg://core/architecture"));
        assert!(uris.contains(&"knogg://state/active_context"));
    }

    #[test]
    fn read_active_context_returns_yaml_text() {
        let root = temp_root("read");
        init(root.to_str().unwrap(), false).unwrap();
        let v = read_resource(&root, "knogg://state/active_context").unwrap();
        let text = v["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("project:"));
        assert_eq!(v["contents"][0]["mimeType"], "text/plain");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn read_architecture_returns_yaml_text() {
        let root = temp_root("arch");
        init(root.to_str().unwrap(), false).unwrap();
        let v = read_resource(&root, "knogg://core/architecture").unwrap();
        assert!(v["contents"][0]["text"].as_str().unwrap().contains("components"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unknown_uri_is_an_error() {
        let root = PathBuf::from("/tmp/unused");
        assert!(read_resource(&root, "knogg://nonexistent").is_err());
    }
}
