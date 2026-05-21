use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::core::vaultio::{atomic_write, backup_file, timestamp, VaultLock};

/// Marker prepended to vault-generated files so they may be safely overwritten.
pub const MARKER: &str = "<!-- generated-by: knogg -->";

/// Valid values for `focus.status`.
pub const ALLOWED_STATUS: [&str; 4] = ["todo", "in_progress", "blocked", "done"];

/// Default `plans/agent_registry.yml`, written by `knogg init`.
pub const DEFAULT_REGISTRY: &str = r#"version: 1
workspace:
  name: knogg
  scope: project
defaults:
  generated_marker: "<!-- generated-by: knogg -->"
  protect_human_files: true
  default_mcp_server: knogg
mcp_servers:
  knogg:
    enabled: true
    transport: stdio
    command: ./knogg
    args:
      - mcp
    env: {}
    description: Local knogg MCP server
agents:
  cursor:
    enabled: true
    kind: cursor
    role: implementer
    outputs:
      mcp_config: .cursor/mcp.json
      instructions: .cursorrules
    mcp_servers:
      - knogg
  claude:
    enabled: true
    kind: claude_code
    role: reviewer
    outputs:
      mcp_config: .mcp.json
      instructions: .claude/context.md
    mcp_servers:
      - knogg
  codex:
    enabled: true
    kind: codex
    role: implementer
    outputs:
      mcp_config: .codex/config.toml
      instructions: AGENTS.md
    mcp_servers:
      - knogg
  opencode:
    enabled: true
    kind: opencode
    role: implementer
    outputs:
      mcp_config: opencode.json
      instructions: AGENTS.md
    mcp_servers:
      - knogg
"#;

/// Default `plans/roles.yml`, written by `knogg init`.
pub const DEFAULT_ROLES: &str = r#"# agent roles: name -> { summary, responsibilities, constraints }
roles:
  implementer:
    summary: Writes and changes code to complete the active task.
    responsibilities:
      - Implement the current focus task
      - Add tests for new behavior
      - Keep the build warning-free
    constraints:
      - Propose state changes, never mutate state directly
  reviewer:
    summary: Reviews proposed changes for correctness and safety.
    responsibilities:
      - Check that tests pass
      - Flag security and path-safety issues
    constraints:
      - Do not apply proposals; recommend only
"#;

/// Default `plans/hooks.yml`, written by `knogg init`.
pub const DEFAULT_HOOKS: &str = r#"# event hooks: event -> { enabled, actions }
hooks:
  before_handoff:
    enabled: true
    actions:
      - refresh_brief
  after_state_change:
    enabled: true
    actions:
      - refresh_brief
      - sync
  after_proposal_apply:
    enabled: true
    actions:
      - refresh_brief
      - sync
  before_mcp_response:
    enabled: true
    actions:
      - ensure_brief_fresh
"#;

// ---- shared MCP helpers ----------------------------------------------------

/// Resolve a vault-relative target, rejecting traversal and absolute paths.
pub fn safe_vault_path(root: &Path, target: &str) -> Result<PathBuf> {
    let p = Path::new(target);
    if p.is_absolute() {
        bail!("absolute paths are not allowed: {target}");
    }
    if target.split(['/', '\\']).any(|c| c == ".." || c == "~") {
        bail!("path traversal is not allowed: {target}");
    }
    let joined = root.join(p);
    if !joined.starts_with(root) {
        bail!("path escapes the vault: {target}");
    }
    Ok(joined)
}

/// Validate a patch. Currently: `focus.status` must be an allowed value.
pub fn audit_patch(patch: &JsonValue) -> Result<()> {
    if let Some(status) = patch.pointer("/focus/status") {
        let s = status
            .as_str()
            .ok_or_else(|| anyhow!("focus.status must be a string"))?;
        if !ALLOWED_STATUS.contains(&s) {
            bail!(
                "invalid focus.status '{s}' (allowed: {})",
                ALLOWED_STATUS.join(", ")
            );
        }
    }
    Ok(())
}

/// Deep-merge `patch` into the YAML file at `target` and write it back.
pub fn apply_patch(root: &Path, target: &str, patch: &JsonValue) -> Result<()> {
    let path = safe_vault_path(root, target)?;
    let raw = fs::read_to_string(&path)
        .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
    let mut doc: serde_yaml::Value = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow!("parsing {}: {e}", path.display()))?;

    let patch_yaml: serde_yaml::Value = serde_yaml::to_value(patch)
        .map_err(|e| anyhow!("converting patch: {e}"))?;
    merge_yaml(&mut doc, &patch_yaml);

    let out = serde_yaml::to_string(&doc).map_err(|e| anyhow!("serializing patched doc: {e}"))?;
    atomic_write(&path, out.as_bytes())?;
    Ok(())
}

/// Recursively merge `patch` into `base` (mappings deep-merge, scalars replace).
fn merge_yaml(base: &mut serde_yaml::Value, patch: &serde_yaml::Value) {
    match (base, patch) {
        (serde_yaml::Value::Mapping(b), serde_yaml::Value::Mapping(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(existing) => merge_yaml(existing, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

// ---- active context model --------------------------------------------------
#[derive(Debug, Serialize, Deserialize)]
pub struct ActiveContext {
    pub project: Project,
    pub focus: Focus,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<String>,
    #[serde(default)]
    pub handoff: Handoff,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Focus {
    pub stage: String,
    pub task: String,
    pub status: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Handoff {
    #[serde(default)]
    pub summary: String,
}

/// Reject paths that try to escape via `..` traversal.
pub fn resolve_path(path: &str) -> Result<PathBuf> {
    if path.split(['/', '\\']).any(|c| c == "..") {
        bail!("path traversal ('..') is not allowed: {path}");
    }
    Ok(PathBuf::from(path))
}

/// Files that make up a freshly initialized vault: (relative path, contents).
fn vault_files() -> Vec<(&'static str, String)> {
    vec![
        (
            "core/index.yml",
            "# vault core index\nfiles: []\n".to_string(),
        ),
        (
            "core/architecture.yml",
            "# architecture overview\ncomponents: []\n".to_string(),
        ),
        (
            "core/style_guides.yml",
            "# style guides\nguides: []\n".to_string(),
        ),
        (
            "state/active_context.yml",
            concat!(
                "project:\n  name: knogg\n",
                "focus:\n  stage: Stage 1\n  task: Implement init & status\n  status: in_progress\n",
                "constraints: []\n",
                "next_actions: []\n",
                "handoff:\n  summary: \"\"\n",
            )
            .to_string(),
        ),
        (
            "state/decision_log.yml",
            "# decision log\ndecisions: []\n".to_string(),
        ),
        (
            "plans/master_plan.yml",
            "# master plan\nstages: []\n".to_string(),
        ),
        (
            "plans/tool_registry.yml",
            concat!(
                "# tool registry: template -> output mappings for `knogg sync`\n",
                "tools:\n",
                "  - name: cursor\n",
                "    template: adapters/cursor_prompt.md\n",
                "    output: .cursorrules\n",
                "  - name: claude\n",
                "    template: adapters/claude_code.md\n",
                "    output: .claude/context.md\n",
                "  - name: codex\n",
                "    template: adapters/codex_prompt.md\n",
                "    output: AGENTS.md\n",
            )
            .to_string(),
        ),
        (
            "plans/agent_registry.yml",
            DEFAULT_REGISTRY.to_string(),
        ),
        (
            "plans/roles.yml",
            DEFAULT_ROLES.to_string(),
        ),
        (
            "plans/hooks.yml",
            DEFAULT_HOOKS.to_string(),
        ),
        (
            "adapters/cursor_prompt.md",
            adapter_template("Cursor"),
        ),
        (
            "adapters/claude_code.md",
            adapter_template("Claude Code"),
        ),
        (
            "adapters/codex_prompt.md",
            adapter_template("Codex"),
        ),
    ]
}

/// Default minijinja handoff template for an agent adapter.
fn adapter_template(agent: &str) -> String {
    format!(
        "{MARKER}\n\
# Handoff → {agent}\n\n\
Project: {{{{ project.name }}}}\n\
Stage: {{{{ focus.stage }}}}\n\
Task: {{{{ focus.task }}}}\n\
Status: {{{{ focus.status }}}}\n\n\
## Constraints\n\
{{% for c in constraints %}}- {{{{ c }}}}\n{{% endfor %}}\n\
## Next Actions\n\
{{% for a in next_actions %}}- {{{{ a }}}}\n{{% endfor %}}\n\
## Summary\n\
{{{{ handoff.summary }}}}\n"
    )
}

/// `knogg init`: create the vault tree and base files.
pub fn init(path: &str, force: bool) -> Result<()> {
    let root = resolve_path(path)?;
    // Serialize against concurrent CLI/MCP/watch writers.
    let _lock = VaultLock::acquire(&root)?;

    for dir in ["core", "state", "plans", "adapters"] {
        fs::create_dir_all(root.join(dir))
            .with_context(|| format!("creating directory {dir}"))?;
    }

    let stamp = timestamp();
    for (rel, contents) in vault_files() {
        let target = root.join(rel);
        if target.exists() {
            if !force {
                bail!(
                    "file already exists (use --force to overwrite): {}",
                    target.display()
                );
            }
            // --force: back up the existing file before overwriting it,
            // but only if its content will actually change.
            let existing = fs::read(&target)
                .with_context(|| format!("reading existing {}", target.display()))?;
            if existing != contents.as_bytes() {
                backup_file(&root, Path::new(rel), &existing, &stamp)?;
            }
        }
        atomic_write(&target, contents.as_bytes())?;
    }

    println!("Vault initialized at {}", root.display());
    Ok(())
}

/// Agent-facing usage guide written by `knogg init --agents-md`.
const AGENTS_MD: &str = r#"<!-- generated-by: knogg -->
# Agent Guide — knogg

This project uses **knogg** to share working context between AI agents and
humans: what is being worked on, what was decided, what to do next.

## MCP server

knogg exposes an MCP server over stdio (JSON-RPC: `initialize`,
`notifications/initialized`, `tools/list`, `tools/call`):

    knogg mcp

## Tools

- `get_active_context` — current project / stage / task / status / next actions.
- `read_vault {path}` — read one vault YAML file.
- `list_vault {include_proposals?}` — list safe vault file paths.
- `search_vault {query}` — case-insensitive text search across the vault.
- `get_tool_registry` — template -> output mappings.
- `list_proposals` — staged proposals and their status.
- `propose_state_update {target, patch, reason}` — stage a state change.
- `audit_commit {id}` — apply a staged proposal.
- `propose_decision {title, reason, status?, scope?}` — record an ADR.
- `post_message {from, text}` / `get_messages {limit?}` — agent message log.

## Workflow

1. Start every task by calling `get_active_context`.
2. Explore with `search_vault` / `read_vault` / `list_vault`.
3. To change state, NEVER write it directly — call `propose_state_update`.
   It stages a `PROP-NNNN` proposal (pending).
4. A human reviews and applies it (`knogg proposal apply <id>`).
5. Check `list_proposals` to see if your proposal was applied or rejected.
6. Record rationale with `propose_decision`.
7. Coordinate with other agents via `post_message` / `get_messages`.

## Rules

- Agents propose; humans apply. No direct state mutation.
- Paths reject `..`; MCP also rejects absolute paths outside the vault.
- Valid `focus.status`: todo | in_progress | blocked | done.
"#;

/// `knogg init --agents-md`: write an `AGENTS.md` agent guide in the cwd.
pub fn write_agents_md(force: bool) -> Result<()> {
    let path = Path::new("AGENTS.md");
    if path.exists() && !force {
        println!("AGENTS.md already exists (use --force to overwrite)");
        return Ok(());
    }
    atomic_write(path, AGENTS_MD.as_bytes())?;
    println!("wrote AGENTS.md");
    Ok(())
}

/// Recommended prompt printed by `knogg init --prompt` to give an AI agent
/// for project setup and knogg configuration.
const SETUP_PROMPT: &str = r#"You are setting up AND building a project with knogg (agent context broker).
Run this in ONE continuous execution — do NOT stop between steps. After each
step run `knogg doctor` to verify. Pause only if a step needs project
knowledge you cannot infer from the repo, or if a command fails twice.

## Step 1 — Project brief

Read the existing project docs (README.md, any plan/ spec files). Then:
1. Set the project name: `knogg state set --stage "Stage 0" --task "Project bootstrap" --status in_progress`
2. Add 3–5 high-level constraints based on the project (tech stack, rules, boundaries).
3. Add the first 3–5 next actions (concrete, actionable items).
4. Record any early decisions: `knogg decision add --title "..." --reason "..." --status accepted`

## Step 2 — Architecture & style

Populate the vault core files:
- `core/index.yml` — list key source directories and their purpose.
- `core/architecture.yml` — list main components/modules and how they connect.
- `core/style_guides.yml` — coding conventions, naming, formatting rules.

## Step 3 — Master plan

Write `plans/master_plan.yml` with staged milestones:
```yaml
stages:
  - name: Stage 0 — Bootstrap
    goal: ...
    tasks: [...]
    status: in_progress
  - name: Stage 1 — ...
    goal: ...
    tasks: [...]
    status: todo
```

## Step 4 — Tool registry

Review `plans/tool_registry.yml`. Add or remove template→output mappings
for the agents actually used in this project. Remove entries for agents
that won't be used.

## Step 5 — Agent registry & roles

Review `plans/agent_registry.yml` and `plans/roles.yml`:
- Enable only the agents you'll use.
- Define roles (e.g. "backend-dev", "reviewer", "test-writer") with
  responsibilities and constraints.
- Assign roles: `knogg agents set-role <agent> <role>`

## Step 6 — AGENTS.md

Run `knogg init --agents-md --force` to write the agent guide at the
project root. Then customize it:
- Add project-specific commands (test, lint, build commands).
- Add the "Structure" section showing the project's directory layout.
- Add the "Standards" section with project conventions.

## Step 7 — Sync & verify

1. Run `knogg sync --dry-run` to preview generated files.
2. Run `knogg sync --force` to write them.
3. Run `knogg doctor` and `knogg agents doctor` to verify.
4. Run `knogg brief show` to see the generated project brief.

## Step 8 — Build loop

Setup is done — now execute the master plan. Loop until every stage in
`plans/master_plan.yml` has `status: done`:
1. `knogg brief show` — read the current focus and next actions.
2. Pick the next `todo`/`in_progress` task in the active stage.
3. Implement it; run the project's test and lint commands.
4. `knogg state set` to advance the task; when all tasks in a stage are
   done, set the stage `done` and move focus to the next stage.
5. Repeat from step 1.

Stop only when all stages are `done`, a command fails twice, or a task
needs project judgement you cannot infer from the repo.
"#;

/// `knogg init --prompt`: print the recommended setup prompt.
pub fn print_setup_prompt() {
    println!("{SETUP_PROMPT}");
}

/// `knogg status`: read and print the active context.
pub fn status(path: &str) -> Result<()> {
    let root = resolve_path(path)?;
    let ctx = read_active_context(&root)?;

    println!("Project: {}", ctx.project.name);
    println!("Stage:   {}", ctx.focus.stage);
    println!("Task:    {}", ctx.focus.task);
    println!("Status:  {}", ctx.focus.status);
    Ok(())
}

/// Load `state/active_context.yml` from a vault root.
pub fn read_active_context(root: &Path) -> Result<ActiveContext> {
    let file = root.join("state/active_context.yml");
    let raw = crate::commands::migrate::read_and_migrate(&file)
        .with_context(|| format!("reading {} (run `knogg init` first?)", file.display()))?;
    serde_yaml::from_str(&raw).map_err(|e| anyhow!("parsing {}: {e}", file.display()))
}

/// Serialize and atomically write the active context. Caller must hold the lock.
pub fn write_active_context(root: &Path, ctx: &ActiveContext) -> Result<()> {
    let file = root.join("state/active_context.yml");
    let out = serde_yaml::to_string(ctx)
        .map_err(|e| anyhow!("serializing active context: {e}"))?;
    atomic_write(&file, out.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vault-test-{label}-{nanos}"))
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(resolve_path("../escape").is_err());
        assert!(resolve_path("./.knogg/../x").is_err());
        assert!(resolve_path("./.knogg").is_ok());
    }

    #[test]
    fn init_creates_full_tree() {
        let root = temp_root("tree");
        let path = root.to_str().unwrap();
        init(path, false).unwrap();

        for dir in ["core", "state", "plans", "adapters"] {
            assert!(root.join(dir).is_dir(), "missing dir {dir}");
        }
        for (rel, _) in vault_files() {
            assert!(root.join(rel).is_file(), "missing file {rel}");
        }
        // Regression (B1/B2): lock released and no atomic-write temp left.
        assert!(!root.join(".lock").exists(), "lock not released after init");
        let leftover_tmp = fs::read_dir(root.join("state"))
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().ends_with(".tmp"));
        assert!(!leftover_tmp, "temp file left behind by init");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn init_fails_without_force_when_exists() {
        let root = temp_root("force");
        let path = root.to_str().unwrap();
        init(path, false).unwrap();

        assert!(init(path, false).is_err(), "should fail without --force");
        assert!(init(path, true).is_ok(), "should succeed with --force");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn init_force_backs_up_changed_files() {
        let root = temp_root("backup");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();

        // Modify a vault file so --force will overwrite it.
        fs::write(
            root.join("state/active_context.yml"),
            "project:\n  name: changed-by-human\nfocus:\n  stage: x\n  task: y\n  status: todo\n",
        )
        .unwrap();
        init(p, true).unwrap();

        let backups = root.join("backups");
        assert!(backups.is_dir(), "no backups directory created");
        let stamp_dir = fs::read_dir(&backups)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let backed = fs::read_to_string(stamp_dir.join("state/active_context.yml")).unwrap();
        assert!(backed.contains("changed-by-human"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn init_force_skips_backup_when_unchanged() {
        let root = temp_root("nobackup");
        let p = root.to_str().unwrap();
        init(p, false).unwrap();
        // Nothing modified -> --force rewrites identical content, no backup.
        init(p, true).unwrap();
        assert!(!root.join("backups").exists(), "backup made for unchanged files");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn agents_md_guide_is_well_formed() {
        assert!(AGENTS_MD.starts_with(MARKER));
        assert!(AGENTS_MD.contains("propose_state_update"));
        assert!(AGENTS_MD.contains("get_active_context"));
        assert!(AGENTS_MD.contains("## Workflow"));
    }

    #[test]
    fn status_reads_active_context() {
        let root = temp_root("status");
        init(root.to_str().unwrap(), false).unwrap();

        let ctx = read_active_context(&root).unwrap();
        assert_eq!(ctx.project.name, "knogg");
        assert_eq!(ctx.focus.stage, "Stage 1");
        assert_eq!(ctx.focus.status, "in_progress");
        fs::remove_dir_all(&root).ok();
    }
}
