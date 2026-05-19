# knogg

**Agent context broker** — a small Rust CLI that keeps a local context store for AI coding agents and brokers that context between tools (Cursor, Claude Code, Codex). Multiple agents and humans share one source of truth for *what is being worked on*, *what was decided*, and *what to do next* — without corrupting files or stepping on each other.

[![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-APACHE2.0-blue)](LICENSE)
[![Docker](https://img.shields.io/badge/Docker-first-2496ED?logo=docker)](docker-compose.yml)
[![MCP](https://img.shields.io/badge/MCP-stdio-black)](README.md#6-mcp-server-for-ai-agents)

## Features

| Feature | Description |
|---------|-------------|
| **Single source of truth** | One `.knogg/` directory per project — state, plans, decisions |
| **Safe writes** | Global lock + atomic rename — crash-safe, no partial files |
| **Staged proposals** | Agents *propose* state changes; humans *apply or reject* |
| **Agent brokering** | Compact handoff prompts rendered per agent (Cursor, Claude, Codex) |
| **MCP server** | JSON-RPC over stdio — agents read context and stage changes programmatically |
| **Template sync** | Generate `.cursorrules`, `AGENTS.md`, `.claude/context.md` from templates |
| **Reactive watch** | Auto-re-sync when the active context changes |

## Quick Start

```bash
# Build release binaries (Docker — no local Rust required)
make release            # → ./dist/knogg + ./dist/knogg.exe

# Initialize the vault
./knogg init

# Check status & integrity
./knogg status
./knogg doctor

# Set focus and sync tool configs
./knogg state set --stage auth --task "Add login" --status in_progress
./knogg sync
./knogg handoff --to cursor --print
```

## Table of Contents

- [Installation](#installation)
- [Vault Layout](#vault-layout)
- [Command Reference](#command-reference)
- [MCP Server](#mcp-server)
- [Workflows](#workflows)
- [Safety Guarantees](#safety-guarantees)
- [Configuration](#configuration)
- [Project Structure](#project-structure)
- [Contributing](#contributing)
- [Known Limitations](#known-limitations)

---

## Installation

### Prerequisites

- **Docker** (Compose v2) — the only runtime dependency
- No local Rust toolchain required

### Build

```bash
# Full release: Unix + Windows cross-compiled into ./dist
make release

# Run tests
make test

# Interactive dev shell
make dev
```

### Three Ways to Run

| Method | Command | When |
|--------|---------|------|
| Wrapper | `./knogg <cmd>` | Daily use — uses `./dist/knogg` if built, else Docker |
| Binary | `./dist/knogg <cmd>` | Fastest — requires `make release` first |
| Docker | `docker compose run --rm dev cargo run -- <cmd>` | CI / no binary built |

> **Tip:** After editing source, rerun `make release` to refresh `./dist/knogg`; otherwise the wrapper runs the stale binary.

---

## Vault Layout

```
.knogg/
├── core/                     # Stable project knowledge
│   ├── index.yml
│   ├── architecture.yml
│   └── style_guides.yml
├── state/                    # Changing state (gitignored)
│   ├── active_context.yml    # Project / focus / constraints / next_actions
│   ├── brief.yml             # Compact brief for MCP tools
│   ├── decision_log.yml      # ADR entries
│   ├── messages.yml          # Inter-agent message log
│   └── proposals/            # Staged proposals (PROP-0001.yml, …)
├── plans/
│   ├── master_plan.yml       # Multi-stage roadmap
│   ├── agent_registry.yml    # Agent definitions & MCP config
│   ├── roles.yml             # Agent role specs
│   ├── tool_registry.yml     # Template → output mappings
│   └── hooks.yml             # Event-driven hooks
└── adapters/                 # Minijinja handoff templates
    ├── cursor_prompt.md
    ├── claude_code.md
    ├── codex_prompt.md
    └── opencode_prompt.md
```

### Active Context

```yaml
project:
  name: knogg
focus:
  stage: Stage 8 — Dynamic workflows
  task: OpenCode: agent capabilities + registry (8D)
  status: in_progress
constraints:
  - "Rust 2021 CLI; build and test via Docker"
  - "All vault writes through knogg (global lock + atomic rename)"
next_actions:
  - "8D (opencode): populate capabilities in agent_registry.yml"
  - "Stage 6 remaining: context profiles per agent in agent_registry"
  - "Run knogg brief doctor after state changes"
  - "8E: make test / doctor / sync verification"
handoff:
  summary: ""
```

---

## Command Reference

Every command accepts `--path <dir>` (defaults to `./.knogg` or `knogg.toml`).

### `knogg init [--force]`

Create the knogg tree and base files.

```bash
knogg init                 # first time
knogg init --force         # regenerate (backs up changed files)
```

### `knogg status`

Print the active context: project, stage, task, status.

```bash
knogg status
# Project: knogg
# Stage:   Stage 8 — Dynamic workflows
# Task:    OpenCode: agent capabilities + registry (8D)
# Status:  in_progress
```

### `knogg doctor`

Diagnose knogg integrity. Reports `[ok]` / `[warn]` / `[error]`, prints `Result: healthy|unhealthy`, exits non-zero on errors.

```bash
knogg doctor
```

### `knogg handoff --to <agent> [--print] [--save <file>]`

Render a compact handoff prompt. Injects only `project.name`, `focus.*`, `constraints`, `next_actions`, `handoff.summary` — **never the full knogg**.

- Agents: `cursor`, `claude`, `codex`
- `--print` and `--save` can be combined

```bash
knogg handoff --to cursor --print
knogg handoff --to claude --save .handoff/claude.md
```

### `knogg sync [--force] [--dry-run]`

Render templates from `plans/tool_registry.yml` to outputs (`.cursorrules`, `.claude/context.md`, `AGENTS.md`).

```bash
knogg sync --dry-run       # preview
knogg sync                 # apply
knogg sync --force         # overwrite human-owned files (with backup)
```

### `knogg state …`

Edit `state/active_context.yml` safely (lock + atomic write).

```bash
knogg state set --stage auth --task "Add login" --status in_progress
knogg state set --status done
knogg state add-next "Update billing page"
knogg state clear-next
```

`--status` must be one of: `todo`, `in_progress`, `blocked`, `done`.

### `knogg decision add`

Append an ADR entry with incremental id (`ADR-0001`, `ADR-0002`, …).

```bash
knogg decision add \
  --title "Use staged proposals for agent changes" \
  --reason "Agents should propose before applying state mutations" \
  --status accepted
```

`--status` must be one of: `proposed`, `accepted`, `rejected`, `superseded`.

### `knogg decision set-status`

Update status on existing ADR(s). Accepts multiple ids.

```bash
knogg decision set-status ADR-0005 ADR-0006 --status accepted
```

### `knogg proposal …`

Manage staged state-change proposals. Supports multiple ids for show/apply/reject.

```bash
knogg proposal list                          # PROP-0001  pending  state/active_context.yml
knogg proposal show PROP-0001 PROP-0002      # multiple ids
knogg proposal apply PROP-0001 PROP-0002     # best-effort batch — not atomic
knogg proposal reject PROP-0001              # mark rejected
knogg proposal gc                            # remove terminal proposals
knogg proposal gc --status applied --keep 5  # keep latest 5 per status
```

### `knogg messages …`

Agent message log for structured coordination.

```bash
knogg messages list                          # all messages
knogg messages list --from cursor --limit 10 # filtered
knogg messages list --unread-for opencode    # unread by agent
knogg messages ack MSG-0001 MSG-0002 --by opencode  # batch ack
```

### `knogg agents …`

Manage agent workspace configuration (MCP configs per agent).

```bash
knogg agents list                            # list agents in registry
knogg agents doctor                          # validate registry
knogg agents inspect                         # show project agent configs
knogg agents sync --dry-run                  # preview config changes
knogg agents set-role cursor builder         # assign role
knogg agents enable opencode                 # enable agent
knogg agents disable codex                   # disable agent
knogg agents enable-mcp cursor knogg         # attach MCP server
knogg agents disable-mcp cursor knogg        # detach MCP server
```

### `knogg role …`

Manage agent role specifications.

```bash
knogg role set builder --summary "Edits Rust code" --responsibility "Implement features"
knogg role list
knogg role show builder
knogg role remove builder
```

### `knogg hooks …`

Manage event-driven hooks.

```bash
knogg hooks list                             # list all hooks
knogg hooks doctor                           # validate hooks
knogg hooks run after_state_change           # execute hook actions
knogg hooks enable after_state_change        # enable hook
knogg hooks disable after_state_change       # disable hook
```

### `knogg brief …`

Manage the compact project brief.

```bash
knogg brief refresh                          # regenerate brief
knogg brief show                             # print brief
knogg brief doctor                           # validate brief
```

### `knogg task …`

Manage partitioned tasks from `plans/master_plan.yml`.

```bash
knogg task list                              # list all tasks
knogg task claim 7A --agent cursor           # claim a task
knogg task release 7A --agent cursor         # release (mark done)
```

### `knogg style …`

Manage coding conventions from `core/style_guides.yml`.

```bash
knogg style list                             # list style guides
knogg style show --lang rust                 # show rules for a language
knogg style doctor                           # check conventions (module docs, fmt)
```

### `knogg mcp`

Run the MCP server (JSON-RPC over stdio). See [MCP Server](#mcp-server).

### `knogg watch`

Watch `state/active_context.yml` and re-run `sync` on change (300–500 ms debounce).

```bash
knogg watch
```

### Command Summary

| Command | Purpose |
|---------|---------|
| `knogg init [--force]` | Create / regenerate the knogg tree |
| `knogg status` | Print project / stage / task / status |
| `knogg doctor` | Diagnose knogg integrity (exit ≠ 0 on error) |
| `knogg handoff --to <agent>` | Render a handoff prompt |
| `knogg sync [--force] [--dry-run]` | Generate tool config files from templates |
| `knogg state set` | Update the active context |
| `knogg state add-next` / `clear-next` | Manage next actions |
| `knogg decision add` / `set-status` | Append ADRs / update ADR status |
| `knogg proposal list/show/apply/reject/gc` | Manage staged proposals |
| `knogg messages list` / `ack` | Agent message log |
| `knogg agents list/sync/set-role/…` | Agent registry management |
| `knogg role set/list/show/remove` | Role specifications |
| `knogg hooks list/run/enable/disable` | Event-driven hooks |
| `knogg brief refresh/show/doctor` | Compact project brief |
| `knogg task list/claim/release` | Partitioned task management |
| `knogg style list/show/doctor` | Coding conventions |
| `knogg mcp` | Run the MCP server (JSON-RPC over stdio) |
| `knogg watch` | Re-run `sync` when the active context changes |

---

## MCP Server

`knogg mcp` speaks **JSON-RPC over stdio** (no HTTP/SSE). One JSON request per line in, one JSON response per line out.

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"get_active_context","params":{}}' \
  | ./knogg mcp
```

### Available Tools

| Tool | Params | Returns |
|------|--------|---------|
| `tools/list` | — | List of available tool names |
| `get_active_context` | `{}` | Resolved `state/active_context.yml` as JSON |
| `get_brief` | `{}` | Compact project brief |
| `get_next_actions` | `{}` | Current next actions |
| `get_allowed_scope` | `{agent?}` | Capability-aware scope for an agent |
| `read_vault` | `{path}` | A vault YAML file as JSON (path-boundary checked) |
| `list_vault` | `{include_proposals?}` | Safe relative paths; hides `backups/`, `.lock`, temp files |
| `search_vault` | `{query}` | Search vault files for text (case-insensitive) |
| `get_tool_registry` | `{}` | Registry mappings — never template contents |
| `list_proposals` | `{}` | Staged proposals and their status |
| `propose_state_update` | `{target, patch, reason}` | Creates a **pending** proposal; does **not** mutate state |
| `propose_decision` | `{title, reason, scope?, status?}` | Append an ADR entry to the decision log |
| `audit_commit` | `{id}` | Applies a staged proposal by id (re-audits first) |
| `post_message` | `{from, text, to?, reply_to?}` | Post a message to the agent message log |
| `get_messages` | `{from?, to?, limit?, status?, unread_for?}` | Read agent messages with optional filters |
| `ack_message` | `{id, by}` | Mark a message read/acked |
| `get_agent_handoff` | `{agent}` | Rendered handoff prompt for an agent |
| `get_agent_role` | `{agent}` | Role spec assigned to an agent |
| `set_agent_role` | `{agent, role}` | Assign a role to an agent |
| `list_roles` | `{}` | List agent roles |
| `get_role` | `{name}` | Show a role by name |
| `set_role` | `{name, summary, responsibilities?, constraints?}` | Create or replace a role |
| `get_current_decisions` | `{}` | Recent decisions from the brief |
| `get_style_guides` | `{}` | Style guides as JSON |

### Example: Propose a State Change

```json
{"jsonrpc":"2.0","id":1,"method":"propose_state_update","params":{
  "target":"state/active_context.yml",
  "patch":{"focus":{"status":"in_progress"}},
  "reason":"Move to frontend UI work"}}
```

The human then applies:

```bash
knogg proposal list
knogg proposal apply PROP-0001
```

### Registering with Agents

Use an **absolute path** to the binary. Build first: `make release` → `./dist/knogg`.

**Cursor** — `.cursor/mcp.json`:
```json
{
  "mcpServers": {
    "knogg": {
      "command": "/ABS/PATH/knogg/dist/knogg",
      "args": ["mcp"]
    }
  }
}
```

**Claude Code** — project `.mcp.json`:
```json
{
  "mcpServers": {
    "knogg": { "command": "/ABS/PATH/knogg/dist/knogg", "args": ["mcp"] }
  }
}
```

**Codex CLI** — `~/.codex/config.toml`:
```toml
[mcp_servers.knogg]
command = "/ABS/PATH/knogg/dist/knogg"
args = ["mcp"]
```

> Restart the agent after editing config. Confirm with a `tools/list` call.

---

## Workflows

### Human: Start Working on a Task

```bash
knogg init                                            # once per project
knogg state set --stage auth --task "Add login" --status in_progress
knogg state add-next "Wire up session cookie"
knogg sync                                            # refresh tool config files
knogg handoff --to cursor --print                     # prompt to paste into Cursor
```

### Human: Record a Decision

```bash
knogg decision add --title "Use JWT sessions" \
  --reason "Stateless, scales horizontally" --status accepted
```

### AI Agent: Read Context, Propose a Change

1. Call `get_active_context` to learn the current focus
2. Call `list_knogg` / `read_knogg` for more detail
3. Call `propose_state_update` — this **stages** `PROP-NNNN`, does *not* change state

The human reviews and decides:

```bash
knogg proposal show PROP-0001
knogg proposal apply PROP-0001     # or: reject
knogg status                       # confirm the change landed
```

### Reactive Sync

```bash
# Terminal 1
knogg watch
# Terminal 2 — any state change triggers `sync` automatically
knogg state set --status done
```

---

## Safety Guarantees

| Guarantee | Implementation |
|-----------|----------------|
| **Global lock** | `.knogg/.lock` — RAII, released on drop, 5s timeout |
| **Atomic writes** | Temp file + rename — crash never leaves partial file |
| **Backups** | `init --force` / `sync --force` back up changed files to `.knogg/backups/<timestamp>/` |
| **Staged proposals** | Agents cannot mutate state directly; changes require human `apply` |
| **Path boundaries** | `..` always rejected; MCP rejects absolute paths and vault escapes |
| **Human files respected** | `sync` never overwrites a file without the generated-by marker unless `--force` |

---

## Configuration

### `knogg.toml`

Place in the project root to avoid passing `--path` every time:

```toml
[knogg]
path = "./.knogg"
generated_marker = "<!-- generated-by: knogg -->"

[features]            # accepted but not yet wired into behavior
clipboard = false
mcp_stdio = true
watch = true

[agents]              # informational
codex_output = "AGENTS.md"
cursor_output = ".cursorrules"
claude_output = ".claude/context.md"
```

### Path Precedence

1. `--path <dir>` CLI flag (highest)
2. `knogg.toml` → `[knogg].path`
3. Default `./.knogg`

---

## Project Structure

```
knogg/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── cli.rs               # clap subcommand definitions
│   ├── mcp/
│   │   └── mod.rs           # JSON-RPC stdio server (initialize + tools/call + legacy)
│   ├── commands/
│   │   ├── agents.rs        # Agent registry, sync, inspect, enable/disable
│   │   ├── brief.rs         # Brief refresh, show, doctor
│   │   ├── decision.rs      # ADR log management (add, set-status)
│   │   ├── doctor.rs        # Integrity diagnostics
│   │   ├── handoff.rs       # Handoff prompt rendering
│   │   ├── hooks.rs         # Event-driven hook execution
│   │   ├── messages.rs      # Agent message log (post, list, ack)
│   │   ├── plan.rs          # Task claim/release from master_plan.yml
│   │   ├── proposal.rs      # Stage/apply/reject/gc proposals (variadic ids)
│   │   ├── roles.rs         # Agent role CRUD
│   │   ├── scope.rs         # Capability-aware allowed scope
│   │   ├── state.rs         # Active context mutations
│   │   ├── style.rs         # Style guide management
│   │   ├── sync.rs          # Template → output generation
│   │   └── watch.rs         # File watcher for reactive sync
│   └── core/
│       ├── config.rs        # knogg.toml parsing, path resolution
│       ├── vault.rs         # Vault init, status, agents_md
│       └── vaultio.rs       # Atomic write, VaultLock, backups
├── .knogg/                  # Vault (see layout above)
├── .github/                 # GitHub CI, issue/PR templates
├── Cargo.toml
├── docker-compose.yml
├── Dockerfile.dev
├── Makefile
├── LICENSE
├── CHANGELOG.md
├── CONTRIBUTING.md
├── SECURITY.md
└── README.md
```

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines on development setup, code style, and the PR process.

Quick start:

```bash
make dev        # enter dev container
make test       # run tests
make release    # build binaries
```

---

## Known Limitations

- **MCP transport is stdio only** — no HTTP / SSE / Streamable HTTP
- **Clipboard is best-effort** — only when the `clipboard` feature is built; otherwise `handoff` falls back to stdout
- **`[features]` / `[agents]` sections** of `knogg.toml` are parsed but not yet wired into behavior
- **Decisions** live in a single `state/decision_log.yml` (no per-ADR files yet)
- **Lock timeout** (5s) may be insufficient for very large vaults under heavy concurrent access

---

## License

Apache License 2.0 — see [LICENSE](LICENSE) and https://www.apache.org/licenses/LICENSE-2.0.
