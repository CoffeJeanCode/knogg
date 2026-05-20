//! `knogg style` — load and enforce conventions from `core/style_guides.yml`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use serde_yaml::Value as YamlValue;

use crate::commands::doctor::{Level, Report};
use crate::core::vault::resolve_path;

/// Deserialize bullet lists where YAML may treat `key: value` lines as maps.
mod rule_lines {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vals = Option::<Vec<YamlValue>>::deserialize(deserializer)?.unwrap_or_default();
        Ok(vals.into_iter().map(value_to_line).collect())
    }

    fn value_to_line(v: YamlValue) -> String {
        match v {
            YamlValue::String(s) => s,
            YamlValue::Mapping(m) => {
                if m.len() == 1 {
                    let (k, val) = m.into_iter().next().unwrap();
                    format!("{}: {}", value_to_scalar(&k), value_to_scalar(&val))
                } else {
                    serde_yaml::to_string(&YamlValue::Mapping(m)).unwrap_or_default()
                }
            }
            YamlValue::Number(n) => n.to_string(),
            YamlValue::Bool(b) => b.to_string(),
            YamlValue::Null => String::new(),
            other => serde_yaml::to_string(&other).unwrap_or_default(),
        }
    }

    fn value_to_scalar(v: &YamlValue) -> String {
        match v {
            YamlValue::String(s) => s.clone(),
            other => value_to_line(other.clone()),
        }
    }
}

/// Relative paths under `src/commands/` that must start with a `//!` module doc.
const COMMAND_MODULES: [&str; 15] = [
    "agents.rs",
    "brief.rs",
    "decision.rs",
    "doctor.rs",
    "handoff.rs",
    "hooks.rs",
    "messages.rs",
    "plan.rs",
    "proposal.rs",
    "roles.rs",
    "scope.rs",
    "state.rs",
    "style.rs",
    "sync.rs",
    "watch.rs",
];

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StyleGuidesFile {
    pub guides: Vec<StyleGuide>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StyleGuide {
    pub lang: String,
    #[serde(default)]
    pub edition: Option<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub formatting: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub naming: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub errors: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub safety: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub tests: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub dependencies: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub docs: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize", rename = "yaml_vault")]
    pub yaml_vault: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub commits: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub lint: Vec<String>,
    #[serde(default, deserialize_with = "rule_lines::deserialize")]
    pub structure: Vec<String>,
}

/// Load `core/style_guides.yml` from the vault.
pub fn load(root: &Path) -> Result<StyleGuidesFile> {
    let file = root.join("core/style_guides.yml");
    let raw = fs::read_to_string(&file)
        .with_context(|| format!("reading {}", file.display()))?;
    serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("parsing {}: {e}", file.display()))
}

/// Append style/convention checks to a doctor report.
pub fn diagnose_conventions(r: &mut Report, vault_root: &Path, check_fmt: bool) {
    match load(vault_root) {
        Ok(doc) if doc.guides.is_empty() => r.warn("style_guides.yml has no guides"),
        Ok(doc) => {
            for g in &doc.guides {
                r.ok(format!(
                    "style guide: {} ({})",
                    g.lang,
                    g.edition.as_deref().unwrap_or("—")
                ));
            }
        }
        Err(e) => r.error(format!("style_guides.yml: {e}")),
    }

    diagnose_command_module_docs(r, vault_root);
    if check_fmt {
        diagnose_cargo_fmt(r, vault_root);
    }
}

fn diagnose_command_module_docs(r: &mut Report, vault_root: &Path) {
    let Some(src_root) = project_src_root(vault_root) else {
        r.warn("could not locate src/commands for module-doc check");
        return;
    };
    let commands_dir = src_root.join("commands");
    if !commands_dir.is_dir() {
        r.warn(format!(
            "missing {} (module-doc check skipped)",
            commands_dir.display()
        ));
        return;
    }

    for name in COMMAND_MODULES {
        let path = commands_dir.join(name);
        if !path.is_file() {
            r.warn(format!("expected command module missing: commands/{name}"));
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(raw) if module_has_doc(&raw) => {
                r.ok(format!("module doc: commands/{name}"));
            }
            Ok(_) => r.warn(format!(
                "commands/{name} missing leading //! module doc (see style_guides docs)"
            )),
            Err(e) => r.warn(format!("cannot read commands/{name}: {e}")),
        }
    }
}

fn module_has_doc(raw: &str) -> bool {
    raw.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim_start().starts_with("//!"))
        .unwrap_or(false)
}

/// Run `cargo fmt --check` when a `Cargo.toml` exists beside the vault.
fn diagnose_cargo_fmt(r: &mut Report, vault_root: &Path) {
    let Some(project_root) = project_root(vault_root) else {
        r.warn("no Cargo.toml beside vault (fmt check skipped)");
        return;
    };
    let output = Command::new("cargo")
        .args(["fmt", "--", "--check"])
        .current_dir(&project_root)
        .output();
    match output {
        Ok(out) if out.status.success() => r.ok("cargo fmt --check"),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let hint = stderr.lines().next().unwrap_or("differs from rustfmt");
            r.error(format!("cargo fmt --check failed: {hint}"));
        }
        Err(e) => r.warn(format!("cargo fmt --check unavailable: {e}")),
    }
}

fn project_root(vault_root: &Path) -> Option<PathBuf> {
    let parent = vault_root.parent()?;
    if parent.join("Cargo.toml").is_file() {
        return Some(parent.to_path_buf());
    }
    None
}

fn project_src_root(vault_root: &Path) -> Option<PathBuf> {
    project_root(vault_root).map(|p| p.join("src"))
}

/// `knogg style list`
pub fn cmd_list(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let doc = load(&root)?;
    if doc.guides.is_empty() {
        println!("(no style guides)");
        return Ok(());
    }
    for g in &doc.guides {
        let edition = g.edition.as_deref().unwrap_or("—");
        println!("{}  edition={edition}", g.lang);
    }
    Ok(())
}

/// `knogg style show --lang rust`
pub fn cmd_show(path: &str, lang: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let doc = load(&root)?;
    let guide = doc
        .guides
        .iter()
        .find(|g| g.lang == lang)
        .ok_or_else(|| anyhow!("no style guide for lang '{lang}'"))?;
    print_section("formatting", &guide.formatting);
    print_section("naming", &guide.naming);
    print_section("errors", &guide.errors);
    print_section("safety", &guide.safety);
    print_section("tests", &guide.tests);
    print_section("dependencies", &guide.dependencies);
    print_section("docs", &guide.docs);
    print_section("yaml_vault", &guide.yaml_vault);
    print_section("commits", &guide.commits);
    print_section("lint", &guide.lint);
    print_section("structure", &guide.structure);
    Ok(())
}

fn print_section(title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    println!("{title}:");
    for line in items {
        println!("  - {line}");
    }
}

/// `knogg style doctor`
pub fn cmd_doctor(path: &str, check_fmt: bool) -> Result<()> {
    let root = resolve_path(path)?;
    let mut r = Report::new();
    diagnose_conventions(&mut r, &root, check_fmt);

    println!("knogg style doctor\n");
    for check in &r.checks {
        let tag = match check.level {
            Level::Ok => "[ok]",
            Level::Warn => "[warn]",
            Level::Error => "[error]",
        };
        println!("{tag} {}", check.message);
    }
    println!();

    if r.has_errors() {
        println!("Result: unhealthy");
        std::process::exit(1);
    }
    println!("Result: healthy");
    Ok(())
}

/// Compact JSON-friendly export for MCP.
#[cfg(test)]
fn guides_json(root: &Path) -> Result<serde_json::Value> {
    let doc = load(root)?;
    serde_json::to_value(&doc).map_err(|e| anyhow!("serializing style guides: {e}"))
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
        std::env::temp_dir().join(format!("vault-style-{label}-{nanos}"))
    }

    #[test]
    fn load_parses_rust_guide() {
        let root = temp_root("load");
        init(root.to_str().unwrap(), false).unwrap();
        fs::write(
            root.join("core/style_guides.yml"),
            "guides:\n  - lang: rust\n    edition: \"2021\"\n    naming:\n      - Modules/files: snake_case\n",
        )
        .unwrap();
        let doc = load(&root).unwrap();
        let rust = doc.guides.iter().find(|g| g.lang == "rust").unwrap();
        assert!(rust.naming.iter().any(|l| l.contains("snake_case")));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn load_real_vault_style_guides() {
        let root = PathBuf::from(".knogg");
        if !root.join("core/style_guides.yml").is_file() {
            return;
        }
        let doc = load(&root).unwrap();
        assert!(doc.guides.iter().any(|g| g.lang == "rust"));
    }

    #[test]
    fn module_doc_detects_missing_header() {
        assert!(module_has_doc("//! ok\nuse std::io;\n"));
        assert!(!module_has_doc("use std::io;\n"));
    }

    #[test]
    fn guides_json_exports() {
        let root = temp_root("json");
        init(root.to_str().unwrap(), false).unwrap();
        let v = guides_json(&root).unwrap();
        assert!(v.get("guides").and_then(|g| g.as_array()).is_some());
        std::fs::remove_dir_all(&root).ok();
    }
}
