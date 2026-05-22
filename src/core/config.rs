use std::collections::HashMap;
use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Primary config file (YAML).
pub const CONFIG_FILE: &str = "knogg.yml";
/// Legacy config file (TOML) — loaded when knogg.yml absent.
const CONFIG_FILE_LEGACY: &str = "knogg.toml";

/// Fallback vault path when neither the CLI flag nor config provide one.
const DEFAULT_PATH: &str = "./.knogg";

/// Parsed `knogg.toml`. Unknown sections (e.g. `[features]`, `[agents]`) are
/// ignored, so the file may carry settings not yet wired into behavior.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub knogg: KnoggSection,
    #[serde(default)]
    pub proposals: ProposalsSection,
    #[serde(default)]
    pub mesh: MeshSection,
}

/// Proposal policy from `knogg.toml` (ADR-0011).
#[derive(Debug, Default, Deserialize)]
pub struct ProposalsSection {
    /// When true, low-risk `active_context` / `brief` patches apply immediately.
    #[serde(default)]
    pub autoapply_low: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct KnoggSection {
    /// Vault directory; used when no `--path` flag is given.
    pub path: Option<String>,
    /// Marker prepended to generated files.
    pub generated_marker: Option<String>,
}

/// P2P mesh section — declarative static peer topology.
#[derive(Debug, Default, Deserialize)]
pub struct MeshSection {
    /// TCP port to listen on for P2P serve.
    pub listen_port: Option<u16>,
    /// Static peer list: name → address.
    #[serde(default)]
    pub peers: HashMap<String, String>,
}

impl Config {
    /// Effective marker for generated files: config value, or the built-in default.
    pub fn generated_marker(&self) -> String {
        self.knogg
            .generated_marker
            .clone()
            .unwrap_or_else(|| crate::core::vault::MARKER.to_string())
    }
}

fn parse_yaml(raw: &str) -> Result<Config> {
    serde_yaml::from_str(raw).context("parsing knogg.yml")
}

fn parse_toml(raw: &str) -> Result<Config> {
    toml::from_str(raw).context("parsing knogg.toml")
}

/// Load config: knogg.yml first, fall back to knogg.toml, then defaults.
pub fn load() -> Result<Config> {
    if let Ok(raw) = fs::read_to_string(CONFIG_FILE) {
        return parse_yaml(&raw);
    }
    if let Ok(raw) = fs::read_to_string(CONFIG_FILE_LEGACY) {
        return parse_toml(&raw);
    }
    Ok(Config::default())
}

/// Resolve the effective vault path.
///
/// Precedence: CLI `--path` flag > `knogg.toml` `[knogg].path` > default.
pub fn resolve_vault_path(cli_path: Option<String>, config: &Config) -> String {
    cli_path
        .or_else(|| config.knogg.path.clone())
        .unwrap_or_else(|| DEFAULT_PATH.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_path(path: Option<&str>) -> Config {
        Config {
            knogg: KnoggSection {
                path: path.map(String::from),
                generated_marker: None,
            },
            proposals: ProposalsSection::default(),
            mesh: MeshSection::default(),
        }
    }

    #[test]
    fn cli_flag_takes_precedence() {
        let cfg = config_with_path(Some("./from-config"));
        let resolved = resolve_vault_path(Some("./from-flag".into()), &cfg);
        assert_eq!(resolved, "./from-flag");
    }

    #[test]
    fn config_used_when_no_flag() {
        let cfg = config_with_path(Some("./from-config"));
        assert_eq!(resolve_vault_path(None, &cfg), "./from-config");
    }

    #[test]
    fn default_used_when_neither() {
        let cfg = config_with_path(None);
        assert_eq!(resolve_vault_path(None, &cfg), DEFAULT_PATH);
    }

    #[test]
    fn parses_full_config_and_ignores_unknown_sections() {
        let raw = r#"
knogg:
  path: ./.knogg
  generated_marker: "<!-- generated-by: knogg -->"

features:
  clipboard: false
  mcp_stdio: true
  watch: true

proposals:
  autoapply_low: true

mesh:
  listen_port: 5050
  peers:
    backend: "tcp://localhost:5051"
    db: "tcp://localhost:5052"

agents:
  codex_output: AGENTS.md
"#;
        let cfg = parse_yaml(raw).unwrap();
        assert_eq!(cfg.knogg.path.as_deref(), Some("./.knogg"));
        assert_eq!(
            cfg.knogg.generated_marker.as_deref(),
            Some("<!-- generated-by: knogg -->")
        );
        assert_eq!(cfg.mesh.listen_port, Some(5050));
        assert_eq!(cfg.mesh.peers.get("backend").as_deref(), Some(&"tcp://localhost:5051".to_string()));
    }

    #[test]
    fn generated_marker_falls_back_to_default() {
        let cfg = config_with_path(None);
        assert_eq!(cfg.generated_marker(), crate::core::vault::MARKER);

        let mut cfg = config_with_path(None);
        cfg.knogg.generated_marker = Some("<!-- custom -->".into());
        assert_eq!(cfg.generated_marker(), "<!-- custom -->");
    }

    #[test]
    fn invalid_yaml_is_an_error() {
        assert!(parse_yaml("key: [unclosed bracket").is_err());
    }

    #[test]
    fn legacy_toml_parses() {
        let raw = r#"
[knogg]
path = "./.knogg"

[proposals]
autoapply_low = true
"#;
        let cfg = parse_toml(raw).unwrap();
        assert_eq!(cfg.knogg.path.as_deref(), Some("./.knogg"));
        assert!(cfg.proposals.autoapply_low);
    }
}
