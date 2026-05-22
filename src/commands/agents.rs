//! `knogg agents` — Agent Workspace Broker.
//!
//! One canonical `plans/agent_registry.yml` describes which agents exist and
//! which MCP servers they use. `agents sync` renders per-agent config files
//! (`.cursor/mcp.json`, `.mcp.json`, `.codex/config.toml`, `opencode.json`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};

use crate::core::vault::resolve_path;
use crate::core::vaultio::{atomic_write, backup_file, timestamp, VaultLock};

/// Agent kinds with a renderer.
const SUPPORTED_KINDS: [&str; 4] = ["cursor", "claude_code", "codex", "opencode"];

/// Suspicious secret prefixes (heuristic).
const SECRET_HINTS: [&str; 4] = ["sk-", "ghp_", "Bearer ", "AKIA"];

// ---- canonical model -------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentRegistry {
    #[serde(default = "default_registry_version")]
    pub version: u32,
    pub workspace: Workspace,
    pub defaults: Defaults,
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServer>,
    #[serde(default)]
    pub agents: BTreeMap<String, Agent>,
}

fn default_registry_version() -> u32 { 1 }

#[derive(Debug, Deserialize, Serialize)]
pub struct Workspace {
    pub name: String,
    pub scope: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Defaults {
    pub generated_marker: String,
    pub protect_human_files: bool,
    pub default_mcp_server: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServer {
    pub enabled: bool,
    pub transport: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Agent {
    pub enabled: bool,
    pub kind: String,
    /// Assigned role name (see `plans/roles.yml`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Context profile: caps how much of the brief this agent receives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<AgentProfile>,
    /// Extra capabilities beyond the assigned role (ADR-0006).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    pub outputs: AgentOutputs,
    #[serde(default)]
    pub mcp_servers: Vec<String>,
}

/// Per-agent context profile — trims the brief to keep prompts small.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_next_actions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_decisions: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentOutputs {
    pub mcp_config: String,
    pub instructions: String,
}

// ---- registry IO -----------------------------------------------------------

fn registry_path(root: &Path) -> PathBuf {
    root.join("plans/agent_registry.yml")
}

/// Load `plans/agent_registry.yml`.
pub fn load_registry(root: &Path) -> Result<AgentRegistry> {
    let path = registry_path(root);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("reading {} (run `knogg init`?)", path.display()))?;
    serde_yaml::from_str(&raw).map_err(|e| anyhow!("parsing {}: {e}", path.display()))
}

/// Serialize and atomically write the registry. Caller must hold the lock.
fn write_registry(root: &Path, reg: &AgentRegistry) -> Result<()> {
    let out = serde_yaml::to_string(reg).map_err(|e| anyhow!("serializing registry: {e}"))?;
    atomic_write(&registry_path(root), out.as_bytes())
}

// ---- shared helpers --------------------------------------------------------

/// Reject `..` traversal and absolute paths in an agent output path.
fn safe_output(out: &str) -> Result<()> {
    if out.split(['/', '\\']).any(|c| c == "..") {
        bail!("output path uses '..': {out}");
    }
    if Path::new(out).is_absolute() {
        bail!("output path is absolute: {out}");
    }
    Ok(())
}

/// Heuristic: does a value look like a literal secret?
fn looks_like_secret(value: &str) -> bool {
    SECRET_HINTS.iter().any(|h| value.contains(h))
}

/// Ownership manifest: output paths vault generated.
///
/// JSON configs (Cursor/Claude/OpenCode) are schema-validated, so vault must
/// not inject marker keys into them. Ownership is tracked out-of-band here
/// instead — a file that exists but is absent from the manifest is treated as
/// human-owned.
#[derive(Debug, Default, Deserialize, Serialize)]
struct OutputManifest {
    #[serde(default)]
    generated: Vec<String>,
}

fn manifest_path(root: &Path) -> PathBuf {
    root.join("state/agent_outputs.yml")
}

fn load_manifest(root: &Path) -> Result<OutputManifest> {
    let path = manifest_path(root);
    match fs::read_to_string(&path) {
        Ok(raw) => serde_yaml::from_str(&raw)
            .map_err(|e| anyhow!("parsing {}: {e}", path.display())),
        Err(_) => Ok(OutputManifest::default()),
    }
}

fn save_manifest(root: &Path, manifest: &OutputManifest) -> Result<()> {
    let out = serde_yaml::to_string(manifest)
        .map_err(|e| anyhow!("serializing output manifest: {e}"))?;
    atomic_write(&manifest_path(root), out.as_bytes())
}

/// A parsed MCP server entry from an existing agent config.
#[derive(Debug, Clone)]
struct ParsedServer {
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

/// Parse the MCP servers declared in an existing config of the given kind.
fn parse_existing_servers(kind: &str, raw: &str) -> Result<BTreeMap<String, ParsedServer>> {
    match kind {
        "cursor" | "claude_code" => parse_json_mcpservers(raw),
        "opencode" => parse_opencode(raw),
        "codex" => parse_codex(raw),
        other => bail!("unsupported agent kind '{other}'"),
    }
}

fn parse_json_mcpservers(raw: &str) -> Result<BTreeMap<String, ParsedServer>> {
    let v: JsonValue = serde_json::from_str(raw)?;
    let mut out = BTreeMap::new();
    if let Some(obj) = v.get("mcpServers").and_then(JsonValue::as_object) {
        for (name, def) in obj {
            out.insert(name.clone(), parsed_from_json(def));
        }
    }
    Ok(out)
}

fn parse_opencode(raw: &str) -> Result<BTreeMap<String, ParsedServer>> {
    let v: JsonValue = serde_json::from_str(raw)?;
    let mut out = BTreeMap::new();
    if let Some(obj) = v.get("mcp").and_then(JsonValue::as_object) {
        for (name, def) in obj {
            // opencode `command` is an array: [cmd, arg, arg, …].
            let cmd = def.get("command").and_then(JsonValue::as_array);
            let (command, args) = match cmd {
                Some(list) => {
                    let mut it = list.iter().filter_map(JsonValue::as_str);
                    let command = it.next().unwrap_or("").to_string();
                    (command, it.map(String::from).collect())
                }
                None => (String::new(), Vec::new()),
            };
            out.insert(
                name.clone(),
                ParsedServer { command, args, env: BTreeMap::new() },
            );
        }
    }
    Ok(out)
}

fn parse_codex(raw: &str) -> Result<BTreeMap<String, ParsedServer>> {
    let v: toml::Value = toml::from_str(raw)?;
    let mut out = BTreeMap::new();
    if let Some(tbl) = v.get("mcp_servers").and_then(toml::Value::as_table) {
        for (name, def) in tbl {
            let command = def
                .get("command")
                .and_then(toml::Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = def
                .get("args")
                .and_then(toml::Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            out.insert(name.clone(), ParsedServer { command, args, env: BTreeMap::new() });
        }
    }
    Ok(out)
}

fn parsed_from_json(def: &JsonValue) -> ParsedServer {
    let command = def
        .get("command")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();
    let args = def
        .get("args")
        .and_then(JsonValue::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let env = def
        .get("env")
        .and_then(JsonValue::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    ParsedServer { command, args, env }
}

// ---- E6: renderers ---------------------------------------------------------

/// Render the MCP config file content for one agent.
pub fn render_agent_config(reg: &AgentRegistry, agent: &Agent) -> Result<String> {
    // Selected = the agent's servers that exist in the registry and are enabled.
    let mut selected: BTreeMap<&str, &McpServer> = BTreeMap::new();
    for name in &agent.mcp_servers {
        if let Some(s) = reg.mcp_servers.get(name) {
            if s.enabled {
                selected.insert(name.as_str(), s);
            }
        }
    }
    match agent.kind.as_str() {
        "cursor" => Ok(render_cursor(&selected)),
        "claude_code" => Ok(render_claude(&selected)),
        "codex" => Ok(render_codex(&selected)),
        "opencode" => Ok(render_opencode(&selected)),
        other => bail!("unsupported agent kind '{other}'"),
    }
}

fn json_env(env: &BTreeMap<String, String>) -> JsonValue {
    let mut m = JsonMap::new();
    for (k, v) in env {
        m.insert(k.clone(), json!(v));
    }
    JsonValue::Object(m)
}

fn render_cursor(selected: &BTreeMap<&str, &McpServer>) -> String {
    let mut servers = JsonMap::new();
    for (name, s) in selected {
        let mut o = JsonMap::new();
        o.insert("command".into(), json!(s.command));
        o.insert("args".into(), json!(s.args));
        if !s.env.is_empty() {
            o.insert("env".into(), json_env(&s.env));
        }
        servers.insert((*name).to_string(), JsonValue::Object(o));
    }
    let doc = json!({"mcpServers": servers});
    format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
}

fn render_claude(selected: &BTreeMap<&str, &McpServer>) -> String {
    let mut servers = JsonMap::new();
    for (name, s) in selected {
        let mut o = JsonMap::new();
        o.insert("type".into(), json!("stdio"));
        o.insert("command".into(), json!(s.command));
        o.insert("args".into(), json!(s.args));
        if !s.env.is_empty() {
            o.insert("env".into(), json_env(&s.env));
        }
        servers.insert((*name).to_string(), JsonValue::Object(o));
    }
    let doc = json!({"mcpServers": servers});
    format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
}

fn render_opencode(selected: &BTreeMap<&str, &McpServer>) -> String {
    let mut servers = JsonMap::new();
    for (name, s) in selected {
        let mut command = vec![json!(s.command)];
        command.extend(s.args.iter().map(|a| json!(a)));
        let mut o = JsonMap::new();
        o.insert("type".into(), json!("local"));
        o.insert("command".into(), JsonValue::Array(command));
        o.insert("enabled".into(), json!(s.enabled));
        if !s.env.is_empty() {
            o.insert("environment".into(), json_env(&s.env));
        }
        servers.insert((*name).to_string(), JsonValue::Object(o));
    }
    let doc = json!({
        "$schema": "https://opencode.ai/config.json",
        "mcp": servers,
    });
    format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
}

fn render_codex(selected: &BTreeMap<&str, &McpServer>) -> String {
    let mut out = String::from("# generated-by: knogg\n");
    for (name, s) in selected {
        out.push_str(&format!("\n[mcp_servers.{name}]\n"));
        out.push_str(&format!("command = {}\n", toml_str(&s.command)));
        let args: Vec<String> = s.args.iter().map(|a| toml_str(a)).collect();
        out.push_str(&format!("args = [{}]\n", args.join(", ")));
        if !s.env.is_empty() {
            out.push_str(&format!("\n[mcp_servers.{name}.env]\n"));
            for (k, v) in &s.env {
                out.push_str(&format!("{k} = {}\n", toml_str(v)));
            }
        }
    }
    out
}

/// Minimal TOML string literal (double-quoted, basic escaping).
fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ---- E2: list & doctor -----------------------------------------------------

pub fn cmd_list(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let reg = load_registry(&root)?;
    for (name, agent) in &reg.agents {
        let state = if agent.enabled { "enabled " } else { "disabled" };
        let servers = if agent.mcp_servers.is_empty() {
            "-".to_string()
        } else {
            agent.mcp_servers.join(",")
        };
        let role = agent.role.as_deref().unwrap_or("-");
        println!(
            "{name:9} {state}  role: {role:11}  mcp: {servers}  output: {}",
            agent.outputs.mcp_config
        );
    }
    Ok(())
}

/// Context profile assigned to an agent (best-effort: `None` on any error).
pub fn agent_profile(root: &Path, agent: &str) -> Option<AgentProfile> {
    load_registry(root)
        .ok()?
        .agents
        .get(agent)?
        .profile
        .clone()
}

/// Role name assigned to an agent.
pub fn agent_role(root: &Path, agent: &str) -> Result<String> {
    let reg = load_registry(root)?;
    let a = reg
        .agents
        .get(agent)
        .ok_or_else(|| anyhow!("unknown agent '{agent}'"))?;
    a.role
        .clone()
        .ok_or_else(|| anyhow!("agent '{agent}' has no role assigned"))
}

/// Assign a role (must exist in `plans/roles.yml`) to an agent.
pub fn set_agent_role(root: &Path, agent: &str, role: &str) -> Result<()> {
    if crate::commands::roles::get(root, role).is_err() {
        bail!("unknown role '{role}' (see `knogg role list`)");
    }
    let _lock = VaultLock::acquire(root)?;
    let mut reg = load_registry(root)?;
    let a = reg
        .agents
        .get_mut(agent)
        .ok_or_else(|| anyhow!("unknown agent '{agent}'"))?;
    a.role = Some(role.to_string());
    write_registry(root, &reg)?;
    Ok(())
}

/// `knogg agents set-role`: assign a role to an agent (CLI wrapper).
pub fn cmd_set_role(path: &str, agent: &str, role: &str) -> Result<()> {
    let root = resolve_path(path)?;
    set_agent_role(&root, agent, role)?;
    println!("agent {agent} role -> {role}");
    Ok(())
}

pub fn cmd_doctor(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let reg = match load_registry(&root) {
        Ok(r) => r,
        Err(e) => {
            println!("Agent doctor\n\n[error] {e}\n\nResult: unhealthy");
            std::process::exit(1);
        }
    };

    println!("Agent doctor\n");
    let mut errors = 0u32;

    for (name, agent) in &reg.agents {
        if SUPPORTED_KINDS.contains(&agent.kind.as_str()) {
            println!("[ok] agent {name} kind '{}'", agent.kind);
        } else {
            println!("[error] agent {name} unsupported kind '{}'", agent.kind);
            errors += 1;
        }
        for server in &agent.mcp_servers {
            if reg.mcp_servers.contains_key(server) {
                println!("[ok] agent {name} -> mcp '{server}'");
            } else {
                println!("[error] agent {name} references unknown mcp '{server}'");
                errors += 1;
            }
        }
        match safe_output(&agent.outputs.mcp_config) {
            Ok(()) => println!("[ok] output {name} -> {}", agent.outputs.mcp_config),
            Err(e) => {
                println!("[error] output {name}: {e}");
                errors += 1;
            }
        }
    }

    for (name, server) in &reg.mcp_servers {
        if server.command.contains('/') && !Path::new(&server.command).exists() {
            println!("[warn] mcp {name} command not found: {}", server.command);
        } else {
            println!("[ok] mcp {name} command '{}'", server.command);
        }
        for (k, v) in &server.env {
            if looks_like_secret(v) {
                println!("[warn] mcp {name} env '{k}' looks like a literal secret");
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

// ---- E3: inspect -----------------------------------------------------------

/// Project config files inspected: (label, kind-or-"", relative path).
const INSPECT_FILES: [(&str, &str, &str); 8] = [
    ("cursor mcp config", "cursor", ".cursor/mcp.json"),
    ("claude mcp config", "claude_code", ".mcp.json"),
    ("codex config", "codex", ".codex/config.toml"),
    ("opencode config", "opencode", "opencode.json"),
    ("opencode config", "opencode", "opencode.jsonc"),
    ("shared instructions", "", ".cursorrules"),
    ("shared instructions", "", ".claude/context.md"),
    ("shared instructions", "", "AGENTS.md"),
];

pub fn cmd_inspect(path: &str) -> Result<()> {
    // path resolution kept for knogg.toml consistency; inspect reads cwd files.
    let _ = resolve_path(path)?;
    println!("Agent config inspection\n");

    let mut servers: BTreeMap<String, ()> = BTreeMap::new();
    for (label, kind, file) in INSPECT_FILES {
        if !Path::new(file).exists() {
            println!("[missing] {label}: {file}");
            continue;
        }
        println!("[found] {label}: {file}");
        if kind.is_empty() {
            continue;
        }
        match fs::read_to_string(file) {
            Ok(raw) => match parse_existing_servers(kind, &raw) {
                Ok(found) => {
                    for name in found.keys() {
                        servers.insert(name.clone(), ());
                    }
                }
                Err(e) => println!("[warn] parse error in {file}: {e}"),
            },
            Err(e) => println!("[warn] cannot read {file}: {e}"),
        }
    }

    println!("\nDetected MCP servers:");
    if servers.is_empty() {
        println!("- (none)");
    } else {
        for name in servers.keys() {
            println!("- {name}");
        }
    }
    Ok(())
}

// ---- E4: diff --------------------------------------------------------------

pub fn cmd_diff(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let reg = load_registry(&root)?;
    let manifest = load_manifest(&root)?;
    println!("Agent config diff\n");
    let mut clean = true;

    for (name, agent) in &reg.agents {
        let mut lines: Vec<String> = Vec::new();
        let out = &agent.outputs.mcp_config;

        if !Path::new(out).exists() {
            lines.push(format!("  missing file: {out}"));
        } else {
            let raw = fs::read_to_string(out)
                .with_context(|| format!("reading {out}"))?;
            if !manifest.generated.iter().any(|g| g == out) {
                lines.push(format!("  human-owned file: {out}"));
            }
            match parse_existing_servers(&agent.kind, &raw) {
                Ok(existing) => {
                    for want in &agent.mcp_servers {
                        match existing.get(want) {
                            None => lines.push(format!("  missing server: {want}")),
                            Some(es) => {
                                if let Some(rs) = reg.mcp_servers.get(want) {
                                    if rs.command != es.command {
                                        lines.push(format!("  mcp {want} differs:"));
                                        lines.push(format!(
                                            "    expected command: {}",
                                            rs.command
                                        ));
                                        lines.push(format!(
                                            "    actual command: {}",
                                            es.command
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    for have in existing.keys() {
                        if !agent.mcp_servers.contains(have) {
                            lines.push(format!("  extra server: {have}"));
                        }
                    }
                }
                Err(e) => lines.push(format!("  parse error: {e}")),
            }
        }

        if !lines.is_empty() {
            clean = false;
            println!("{name}:");
            for l in lines {
                println!("{l}");
            }
            println!();
        }
    }

    if clean {
        println!("all agents aligned");
    }
    Ok(())
}

// ---- E5: generalize --------------------------------------------------------

pub fn cmd_generalize(path: &str, from: &str, force: bool) -> Result<()> {
    let root = resolve_path(path)?;
    let _lock = VaultLock::acquire(&root)?;
    let mut reg = load_registry(&root)?;

    let agent = reg
        .agents
        .get(from)
        .ok_or_else(|| anyhow!("unknown agent '{from}'"))?;
    let kind = agent.kind.clone();
    let source = agent.outputs.mcp_config.clone();

    let raw = fs::read_to_string(&source)
        .with_context(|| format!("reading {source} (config not present?)"))?;
    let found = parse_existing_servers(&kind, &raw)?;

    let mut added = 0u32;
    for (name, ps) in found {
        if reg.mcp_servers.contains_key(&name) && !force {
            println!("skip {name}: already in registry (use --force to overwrite)");
            continue;
        }
        if looks_like_secret(&ps.command)
            || ps.args.iter().any(|a| looks_like_secret(a))
            || ps.env.values().any(|v| looks_like_secret(v))
        {
            println!("warn {name}: value looks like a literal secret");
        }
        reg.mcp_servers.insert(
            name.clone(),
            McpServer {
                enabled: true,
                transport: "stdio".to_string(),
                command: ps.command,
                args: ps.args,
                env: ps.env,
                description: format!("generalized from {from}"),
            },
        );
        added += 1;
        println!("added mcp {name}");
    }

    if added > 0 {
        if force {
            let stamp = timestamp();
            backup_file(
                &root,
                Path::new("plans/agent_registry.yml"),
                fs::read(registry_path(&root))?.as_slice(),
                &stamp,
            )?;
        }
        write_registry(&root, &reg)?;
    }
    println!("generalized {added} server(s) from {from}");
    Ok(())
}

// ---- E7/E8: sync -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Create,
    Update,
    Unchanged,
    SkipHuman,
    Disabled,
}

pub fn sync(path: &str, force: bool, dry_run: bool) -> Result<()> {
    let root = resolve_path(path)?;
    let reg = load_registry(&root)?;
    // dry-run writes nothing, so it takes no lock.
    let _lock = if dry_run {
        None
    } else {
        Some(VaultLock::acquire(&root)?)
    };
    let stamp = timestamp();
    let mut manifest = load_manifest(&root)?;
    let mut manifest_dirty = false;

    for (name, agent) in &reg.agents {
        let out = &agent.outputs.mcp_config;
        if let Err(e) = safe_output(out) {
            bail!("agent {name}: {e}");
        }

        if !agent.enabled {
            report(dry_run, Action::Disabled, name, out);
            continue;
        }

        let content = render_agent_config(&reg, agent)?;
        let output_path = PathBuf::from(out);
        let owned = manifest.generated.iter().any(|g| g == out);

        let existing: Option<String> = if output_path.exists() {
            Some(fs::read_to_string(&output_path).with_context(|| format!("reading {out}"))?)
        } else {
            None
        };
        // A file that exists but vault never generated is human-owned.
        let action = match &existing {
            None => Action::Create,
            Some(_) if !owned && !force => Action::SkipHuman,
            Some(e) if *e == content => Action::Unchanged,
            Some(_) => Action::Update,
        };

        if dry_run {
            report(true, action, name, out);
            continue;
        }

        match action {
            Action::Disabled | Action::SkipHuman => {
                report(false, action, name, out);
                continue;
            }
            Action::Unchanged => {}
            Action::Create | Action::Update => {
                if action == Action::Update && force && !owned {
                    if let Some(old) = &existing {
                        backup_file(&root, &output_path, old.as_bytes(), &stamp)?;
                    }
                }
                atomic_write(&output_path, content.as_bytes())?;
            }
        }
        report(false, action, name, out);
        // File now holds vault content — record ownership.
        if !owned {
            manifest.generated.push(out.clone());
            manifest_dirty = true;
        }
    }

    if !dry_run && manifest_dirty {
        manifest.generated.sort();
        manifest.generated.dedup();
        save_manifest(&root, &manifest)?;
    }
    Ok(())
}

fn report(dry_run: bool, action: Action, name: &str, out: &str) {
    let msg = match (dry_run, action) {
        (_, Action::Disabled) => format!("{name} skipped disabled"),
        (true, Action::Create) => format!("would create {out}"),
        (true, Action::Update) => format!("would update {out}"),
        (true, Action::Unchanged) => format!("unchanged {out}"),
        (true, Action::SkipHuman) => format!("would skip {out} human-owned"),
        (false, Action::Create) => format!("created {out}"),
        (false, Action::Update) => format!("updated {out}"),
        (false, Action::Unchanged) => format!("unchanged {out}"),
        (false, Action::SkipHuman) => {
            format!("skipped {out} human-owned (use --force)")
        }
    };
    println!("{msg}");
}

// ---- E9: enable / disable --------------------------------------------------

pub fn set_agent_enabled(path: &str, agent: &str, enabled: bool) -> Result<()> {
    let root = resolve_path(path)?;
    let _lock = VaultLock::acquire(&root)?;
    let mut reg = load_registry(&root)?;
    let a = reg
        .agents
        .get_mut(agent)
        .ok_or_else(|| anyhow!("unknown agent '{agent}'"))?;
    a.enabled = enabled;
    write_registry(&root, &reg)?;
    println!("agent {agent} {}", if enabled { "enabled" } else { "disabled" });
    Ok(())
}

pub fn set_agent_mcp(path: &str, agent: &str, server: &str, enabled: bool) -> Result<()> {
    let root = resolve_path(path)?;
    let _lock = VaultLock::acquire(&root)?;
    let mut reg = load_registry(&root)?;
    if enabled && !reg.mcp_servers.contains_key(server) {
        bail!("unknown mcp server '{server}'");
    }
    let a = reg
        .agents
        .get_mut(agent)
        .ok_or_else(|| anyhow!("unknown agent '{agent}'"))?;
    if enabled {
        if !a.mcp_servers.iter().any(|s| s == server) {
            a.mcp_servers.push(server.to_string());
        }
    } else {
        a.mcp_servers.retain(|s| s != server);
    }
    write_registry(&root, &reg)?;
    println!(
        "agent {agent} mcp {server} {}",
        if enabled { "enabled" } else { "disabled" }
    );
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
        std::env::temp_dir().join(format!("vault-agents-{label}-{nanos}"))
    }

    fn with_cwd<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        let out = f();
        std::env::set_current_dir(prev).unwrap();
        out
    }

    #[test]
    fn default_registry_parses() {
        let reg: AgentRegistry = serde_yaml::from_str(crate::core::vault::DEFAULT_REGISTRY).unwrap();
        assert_eq!(reg.version, 1);
        assert!(reg.mcp_servers.contains_key("knogg"));
        for a in ["cursor", "claude", "codex", "opencode"] {
            assert!(reg.agents.contains_key(a), "missing agent {a}");
        }
    }

    #[test]
    fn init_writes_agent_registry() {
        let root = temp_root("init");
        init(root.to_str().unwrap(), false).unwrap();
        let reg = load_registry(&root).unwrap();
        assert_eq!(reg.agents.len(), 4);
    }

    #[test]
    fn renderers_emit_expected_formats() {
        let reg: AgentRegistry = serde_yaml::from_str(crate::core::vault::DEFAULT_REGISTRY).unwrap();
        let cursor = render_agent_config(&reg, &reg.agents["cursor"]).unwrap();
        assert!(cursor.contains("\"mcpServers\""));
        // JSON configs carry no marker key (would break schema validation).
        assert!(!cursor.contains("_generated_by"));

        let claude = render_agent_config(&reg, &reg.agents["claude"]).unwrap();
        assert!(claude.contains("\"type\": \"stdio\""));
        assert!(!claude.contains("_generated_by"));

        let codex = render_agent_config(&reg, &reg.agents["codex"]).unwrap();
        assert!(codex.contains("# generated-by: knogg"));
        assert!(codex.contains("[mcp_servers.knogg]"));

        let oc = render_agent_config(&reg, &reg.agents["opencode"]).unwrap();
        assert!(oc.contains("\"$schema\""));
        assert!(oc.contains("\"type\": \"local\""));
    }

    #[test]
    fn secret_heuristic_flags_tokens() {
        assert!(looks_like_secret("ghp_abc123"));
        assert!(looks_like_secret("sk-xxxx"));
        assert!(!looks_like_secret("./knogg"));
    }

    #[test]
    fn sync_dry_run_writes_nothing_then_sync_creates() {
        let root = temp_root("sync");
        fs::create_dir_all(&root).unwrap();
        init(root.join(".knogg").to_str().unwrap(), false).unwrap();

        with_cwd(&root, || {
            sync("./.knogg", false, true).unwrap();
            assert!(!root.join(".cursor/mcp.json").exists());

            sync("./.knogg", false, false).unwrap();
            assert!(root.join(".cursor/mcp.json").is_file());
            assert!(root.join(".mcp.json").is_file());
            assert!(root.join(".codex/config.toml").is_file());
            assert!(root.join("opencode.json").is_file());

            // Idempotent.
            let before = fs::read_to_string(root.join(".cursor/mcp.json")).unwrap();
            sync("./.knogg", false, false).unwrap();
            assert_eq!(fs::read_to_string(root.join(".cursor/mcp.json")).unwrap(), before);
        });
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn human_owned_json_is_protected_via_manifest() {
        let root = temp_root("humanowned");
        fs::create_dir_all(&root).unwrap();
        let vault = root.join(".knogg");
        init(vault.to_str().unwrap(), false).unwrap();
        // Pre-existing human file, never generated by vault.
        fs::create_dir_all(root.join(".cursor")).unwrap();
        fs::write(root.join(".cursor/mcp.json"), "{\"hand\":\"written\"}").unwrap();

        with_cwd(&root, || {
            sync("./.knogg", false, false).unwrap();
            // Untouched: no manifest entry -> human-owned.
            assert_eq!(
                fs::read_to_string(root.join(".cursor/mcp.json")).unwrap(),
                "{\"hand\":\"written\"}"
            );
            // --force overwrites and backs up.
            sync("./.knogg", true, false).unwrap();
            assert!(fs::read_to_string(root.join(".cursor/mcp.json"))
                .unwrap()
                .contains("mcpServers"));
            assert!(vault.join("backups").is_dir());
        });
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn disabled_agent_is_skipped_by_sync() {
        let root = temp_root("disabled");
        fs::create_dir_all(&root).unwrap();
        let vault = root.join(".knogg");
        init(vault.to_str().unwrap(), false).unwrap();
        set_agent_enabled(vault.to_str().unwrap(), "opencode", false).unwrap();

        with_cwd(&root, || {
            sync("./.knogg", false, false).unwrap();
            assert!(!root.join("opencode.json").exists());
            assert!(root.join(".cursor/mcp.json").is_file());
        });
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn generalize_adds_servers_from_cursor() {
        let root = temp_root("generalize");
        fs::create_dir_all(&root).unwrap();
        let vault = root.join(".knogg");
        init(vault.to_str().unwrap(), false).unwrap();
        fs::create_dir_all(root.join(".cursor")).unwrap();
        fs::write(
            root.join(".cursor/mcp.json"),
            r#"{"mcpServers":{"context7":{"command":"npx","args":["-y","@upstash/context7-mcp"]}}}"#,
        )
        .unwrap();

        with_cwd(&root, || {
            cmd_generalize("./.knogg", "cursor", false).unwrap();
        });
        let reg = load_registry(&vault).unwrap();
        assert!(reg.mcp_servers.contains_key("context7"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn enable_disable_mcp_updates_registry() {
        let root = temp_root("mcp");
        let vault = root.join(".knogg");
        init(vault.to_str().unwrap(), false).unwrap();
        let p = vault.to_str().unwrap();

        set_agent_mcp(p, "cursor", "knogg", false).unwrap();
        assert!(load_registry(&vault).unwrap().agents["cursor"]
            .mcp_servers
            .is_empty());
        set_agent_mcp(p, "cursor", "knogg", true).unwrap();
        assert_eq!(
            load_registry(&vault).unwrap().agents["cursor"].mcp_servers,
            vec!["knogg".to_string()]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn agent_role_default_and_reassign() {
        let root = temp_root("agentrole");
        init(root.to_str().unwrap(), false).unwrap();

        // Seeded defaults: claude reviews, opencode implements.
        assert_eq!(agent_role(&root, "claude").unwrap(), "reviewer");
        assert_eq!(agent_role(&root, "opencode").unwrap(), "implementer");

        // Reassign to an existing role.
        set_agent_role(&root, "claude", "implementer").unwrap();
        assert_eq!(agent_role(&root, "claude").unwrap(), "implementer");

        // Unknown role / unknown agent are rejected.
        assert!(set_agent_role(&root, "claude", "ghost").is_err());
        assert!(set_agent_role(&root, "ghost", "reviewer").is_err());
        fs::remove_dir_all(&root).ok();
    }
}
