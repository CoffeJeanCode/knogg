use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::vault::resolve_path;

/// Required directories in a healthy vault.
const REQUIRED_DIRS: [&str; 4] = ["core", "state", "plans", "adapters"];

/// Files every initialized vault must contain.
const REQUIRED_FILES: [&str; 13] = [
    "core/index.yml",
    "core/architecture.yml",
    "core/style_guides.yml",
    "state/active_context.yml",
    "state/decision_log.yml",
    "plans/master_plan.yml",
    "plans/tool_registry.yml",
    "plans/agent_registry.yml",
    "plans/roles.yml",
    "plans/hooks.yml",
    "adapters/cursor_prompt.md",
    "adapters/claude_code.md",
    "adapters/codex_prompt.md",
];

/// YAML files that must parse cleanly.
const PARSEABLE: [&str; 6] = [
    "state/active_context.yml",
    "state/decision_log.yml",
    "plans/tool_registry.yml",
    "plans/agent_registry.yml",
    "plans/roles.yml",
    "plans/hooks.yml",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Error,
}

pub struct Check {
    pub level: Level,
    pub message: String,
}

pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    fn new() -> Self {
        Report { checks: Vec::new() }
    }
    fn ok(&mut self, m: impl Into<String>) {
        self.checks.push(Check { level: Level::Ok, message: m.into() });
    }
    fn warn(&mut self, m: impl Into<String>) {
        self.checks.push(Check { level: Level::Warn, message: m.into() });
    }
    fn error(&mut self, m: impl Into<String>) {
        self.checks.push(Check { level: Level::Error, message: m.into() });
    }
    pub fn has_errors(&self) -> bool {
        self.checks.iter().any(|c| c.level == Level::Error)
    }
}

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

/// Inspect a vault and collect health checks (pure: no process exit).
///
/// `marker` is the generated-by marker (from `knogg.toml`, or the default).
pub fn diagnose(path: &str, marker: &str) -> Report {
    let mut r = Report::new();

    let root = match resolve_path(path) {
        Ok(p) => p,
        Err(e) => {
            r.error(format!("invalid vault path: {e}"));
            return r;
        }
    };

    if !root.is_dir() {
        r.error(format!(
            "{} does not exist (run `knogg init` first?)",
            root.display()
        ));
        return r;
    }

    for dir in REQUIRED_DIRS {
        if root.join(dir).is_dir() {
            r.ok(format!("dir {dir}/"));
        } else {
            r.error(format!("missing dir {dir}/"));
        }
    }

    for file in REQUIRED_FILES {
        if root.join(file).is_file() {
            r.ok(file.to_string());
        } else {
            r.error(format!("missing file {file}"));
        }
    }

    for file in PARSEABLE {
        match fs::read_to_string(root.join(file)) {
            Ok(raw) => match serde_yaml::from_str::<serde_yaml::Value>(&raw) {
                Ok(_) => r.ok(format!("parsed {file}")),
                Err(e) => r.error(format!("parse error in {file}: {e}")),
            },
            Err(e) => r.error(format!("cannot read {file}: {e}")),
        }
    }

    diagnose_registry(&mut r, &root, marker);
    r
}

/// Validate the templates and outputs declared in `tool_registry.yml`.
fn diagnose_registry(r: &mut Report, root: &Path, marker: &str) {
    let registry_path = root.join("plans/tool_registry.yml");
    let raw = match fs::read_to_string(&registry_path) {
        Ok(raw) => raw,
        Err(_) => return, // already reported as a missing/unreadable file
    };
    let registry: ToolRegistry = match serde_yaml::from_str(&raw) {
        Ok(reg) => reg,
        Err(_) => return, // already reported as a parse error
    };

    for tool in &registry.tools {
        // Declared template must exist inside the vault.
        if root.join(&tool.template).is_file() {
            r.ok(format!("adapter {} -> {}", tool.name, tool.template));
        } else {
            r.error(format!(
                "adapter {} -> {} missing",
                tool.name, tool.template
            ));
        }
        check_output(r, &tool.name, &tool.output, marker);
    }
}

/// Validate a single registry output path.
fn check_output(r: &mut Report, name: &str, output: &str, marker: &str) {
    if output.split(['/', '\\']).any(|c| c == "..") {
        r.error(format!("output {name} -> {output} uses '..' (path traversal)"));
        return;
    }
    if Path::new(output).is_absolute() {
        r.error(format!("output {name} -> {output} is an absolute path"));
        return;
    }

    let path = Path::new(output);
    if !path.exists() {
        r.ok(format!("output {name} -> {output} (not yet generated)"));
        return;
    }
    match fs::read_to_string(path) {
        Ok(content) if content.contains(marker) => {
            r.ok(format!("output {name} -> {output}"));
        }
        Ok(_) => {
            r.warn(format!("output {name} -> {output} human-owned (no marker)"));
        }
        Err(e) => {
            r.error(format!("output {name} -> {output} unreadable: {e}"));
        }
    }
}

/// `knogg doctor`: print the report and exit non-zero if anything failed.
pub fn doctor(path: &str, marker: &str) -> Result<()> {
    let report = diagnose(path, marker);

    println!("knogg doctor\n");
    for check in &report.checks {
        let tag = match check.level {
            Level::Ok => "[ok]",
            Level::Warn => "[warn]",
            Level::Error => "[error]",
        };
        println!("{tag} {}", check.message);
    }
    println!();

    if report.has_errors() {
        println!("Result: unhealthy");
        std::process::exit(1);
    }
    println!("Result: healthy");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{init, MARKER};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vault-doctor-{label}-{nanos}"))
    }

    #[test]
    fn fresh_vault_is_healthy() {
        let root = temp_root("healthy");
        init(root.to_str().unwrap(), false).unwrap();
        let report = diagnose(root.to_str().unwrap(), MARKER);
        assert!(!report.has_errors(), "fresh vault should have no errors");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_file_is_reported() {
        let root = temp_root("missing");
        init(root.to_str().unwrap(), false).unwrap();
        fs::remove_file(root.join("state/decision_log.yml")).unwrap();

        let report = diagnose(root.to_str().unwrap(), MARKER);
        assert!(report.has_errors());
        assert!(report
            .checks
            .iter()
            .any(|c| c.level == Level::Error && c.message.contains("decision_log.yml")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_vault_is_reported() {
        let root = temp_root("absent");
        let report = diagnose(root.to_str().unwrap(), MARKER);
        assert!(report.has_errors());
    }

    #[test]
    fn traversal_output_is_rejected() {
        let root = temp_root("traversal");
        init(root.to_str().unwrap(), false).unwrap();
        fs::write(
            root.join("plans/tool_registry.yml"),
            "tools:\n  - name: evil\n    template: adapters/cursor_prompt.md\n    output: ../escape\n",
        )
        .unwrap();

        let report = diagnose(root.to_str().unwrap(), MARKER);
        assert!(report.has_errors());
        assert!(report
            .checks
            .iter()
            .any(|c| c.level == Level::Error && c.message.contains("path traversal")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn absolute_output_is_rejected() {
        let mut r = Report::new();
        check_output(&mut r, "x", "/etc/passwd", MARKER);
        assert!(r.has_errors());
    }
}
