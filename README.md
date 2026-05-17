# knogg — agent context broker

`knogg` is a small Rust CLI that keeps a **local context store** for AI coding
agents and brokers that context between tools (Cursor, Claude Code, Codex /
CLI agents). It lets multiple agents and humans share one source of truth for
*what is being worked on*, *what was decided*, and *what to do next* — without
corrupting files or stepping on each other.

- **Single source of truth** — one `.knogg/` directory per project.
- **Safe writes** — every write goes through a global lock + atomic rename.
- **Agent brokering** — compact handoff prompts and an MCP server over stdio.
- **Staged changes** — agents *propose*; humans *apply or reject*.

---

## 1. Quick start

```bash
# build the local binary (Docker, no local Rust required)
make release            # produces ./dist/knogg

# everyday use via the wrapper (uses ./dist/knogg if present, else Docker)
./knogg init            # create ./.knogg
./knogg status          # show current focus
./knogg doctor          # check knogg integrity
```

There is **no local Rust toolchain**. Everything builds and runs inside the
`dev` service of `docker-compose`.

---

## 2. Installation & execution

### 2.1 Build / test (Docker)

```bash
docker compose run --rm dev cargo test            # run the test suite
docker compose run --rm dev cargo build --release # build (binary stays in a volume)
make release                                      # build Unix + Windows into ./dist
```

`make release` is the supported way to get host-visible binaries. `./target`
lives in a Docker volume, so `make release` builds both targets and copies the
executables out:

- `./dist/knogg` — Unix (Linux x86_64)
- `./dist/knogg.exe` — Windows (x86_64, cross-compiled via mingw-w64)

First run rebuilds the `dev` image to add the Windows toolchain (slow once).

### 2.2 Three ways to run


| Method         | Command                                          | When to use                                                         |
| -------------- | ------------------------------------------------ | ------------------------------------------------------------------- |
| Wrapper        | `./knogg <cmd>`                                  | Daily use. Runs `./dist/knogg` if built, else falls back to Docker. |
| Release binary | `./dist/knogg <cmd>`                             | Direct, fastest. Requires `make release` first.                     |
| Docker         | `docker compose run --rm dev cargo run -- <cmd>` | CI / no binary built. Add `-T` when piping stdin (MCP).             |


All examples below use `knogg <cmd>` — substitute whichever method you prefer.

### 2.3 Working with the `./knogg` script

`./knogg` is a wrapper script in the project root. Logic:

```sh
if [ -x ./dist/knogg ]; then exec ./dist/knogg "$@"   # fast native binary
else exec docker compose run --rm dev cargo run -- "$@"  # fallback
fi
```

- No local Rust needed. No binary built → it falls back to Docker.
- Every arg after `./knogg` passes straight to the CLI.

```bash
./knogg init                 # create the knogg
./knogg status               # show focus
./knogg sync --dry-run       # preview
./knogg doctor               # integrity check
./knogg state set --status done
./knogg --help               # full command list
```

Make it faster — build once, then the wrapper uses the binary:

```bash
make release                 # build ./dist/knogg
./knogg status               # now runs the native binary, no Docker
```

Piping stdin (MCP) through the wrapper:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' \
  | ./knogg mcp
```

Notes:
- After editing source, rerun `make release` to refresh `./dist/knogg`;
  otherwise the wrapper runs the stale binary.
- `./knogg` runs as your host user. A `.knogg/` created earlier by Docker
  (root-owned) may need `sudo chown -R $USER .knogg` before the wrapper can
  write to it.

---

## 3. Configuration & path resolution

### 3.1 `knogg.toml`

Place a `knogg.toml` in the project root to avoid passing `--path` every time:

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

- `path` — knogg directory.
- `generated_marker` — marker line used by `sync`/`doctor` to tell
knogg-generated files apart from human-owned ones.
- Unknown sections (`[features]`, `[agents]`) are accepted and ignored.

### 3.2 Path precedence

1. `--path <dir>` CLI flag (highest priority)
2. `knogg.toml` → `[knogg].path`
3. Default `./.knogg`

### 3.3 Path safety

Paths containing `..` are rejected everywhere. MCP additionally rejects
absolute paths and anything that escapes the knogg root.

---

## 4. Vault layout

`knogg init` creates:

```
.knogg/
├── core/                     # stable project knowledge
│   ├── index.yml
│   ├── architecture.yml
│   └── style_guides.yml
├── state/                    # changing state
│   ├── active_context.yml    # project / focus / constraints / next_actions / handoff
│   ├── decision_log.yml      # ADR entries
│   └── proposals/            # staged proposals (PROP-0001.yml, …) — created on demand
├── plans/
│   ├── master_plan.yml
│   └── tool_registry.yml     # template → output mappings used by `sync`
├── adapters/                 # minijinja handoff templates per agent
│   ├── cursor_prompt.md
│   ├── claude_code.md
│   └── codex_prompt.md
└── backups/                  # created on demand: backups/<timestamp>/<file>
```

`active_context.yml` shape:

```yaml
project:
  name: knogg
focus:
  stage: Stage 1
  task: Implement init & status
  status: in_progress          # todo | in_progress | blocked | done
constraints: []
next_actions: []
handoff:
  summary: ""
```

---

## 5. Command reference

Every command accepts `--path <dir>` (optional — see §3.2).

### `knogg init [--force]`

Create the knogg tree and base files.

- Fails if the knogg already exists — pass `--force` to regenerate.
- Under `--force`, any file whose content changes is first backed up to
`.knogg/backups/<timestamp>/`. Unchanged files are not backed up.

```bash
knogg init                 # first time
knogg init --force         # regenerate (backs up changed files)
```

### `knogg status`

Print the active context: project, stage, task, status.

```bash
knogg status
# Project: knogg
# Stage:   frontend-ui
# Task:    Implement subscription badge
# Status:  in_progress
```

### `knogg doctor`

Diagnose knogg integrity. Reports `[ok]` / `[warn]` / `[error]`, prints
`Result: healthy|unhealthy`, and exits non-zero if any error is found.

Checks: required dirs and files exist; `active_context.yml`,
`decision_log.yml`, `tool_registry.yml` parse; every registry template
exists; registry outputs use no `../` and no absolute paths; existing
generated outputs are marker-tagged (`[warn] human-owned` otherwise).

```bash
knogg doctor
```

### `knogg handoff --to <agent> [--print] [--save <file>]`

Render a compact handoff prompt from `adapters/<agent>` + the active context.
Injects only `project.name`, `focus.*`, `constraints`, `next_actions`,
`handoff.summary` — **never the full knogg**.

- Agents: `cursor`, `claude`, `codex`.
- `--print` — write to stdout.
- `--save <file>` — write to a file (parent dirs created; existing file
overwritten — it is an explicit output).
- `--print` and `--save` can be combined.
- With neither flag: copy to clipboard if the `clipboard` feature is built,
else fall back to stdout.

```bash
knogg handoff --to cursor --print
knogg handoff --to claude --save .handoff/claude.md
```

### `knogg sync [--force] [--dry-run]`

Render the templates in `plans/tool_registry.yml` to their outputs
(`.cursorrules`, `.claude/context.md`, `AGENTS.md`). Output is prefixed with
the generated-by marker.

- Idempotent — unchanged outputs are not rewritten.
- A file lacking the marker is treated as human-owned and skipped unless
`--force` (which first backs the file up).
- `--dry-run` — show the plan, write nothing, take no lock:
`would create` / `would update` / `would skip … human-owned` / `unchanged`.

```bash
knogg sync --dry-run       # preview
knogg sync                 # apply
knogg sync --force         # also overwrite human-owned files (with backup)
```

### `knogg state …`

Edit `state/active_context.yml` safely (lock + atomic write).

```bash
knogg state set --stage frontend-ui --task "Implement badge" --status in_progress
knogg state set --status done                 # any subset of fields
knogg state add-next "Update billing page"    # append a next action
knogg state clear-next                        # remove all next actions
```

`--status` must be one of `todo`, `in_progress`, `blocked`, `done`.

### `knogg decision add`

Append an ADR entry to `state/decision_log.yml` with an incremental id
(`ADR-0001`, `ADR-0002`, …) and today's date.

```bash
knogg decision add \
  --title "Use staged proposals for agent changes" \
  --reason "Agents should propose before applying state mutations" \
  --status accepted \
  --scope global          # --scope defaults to "global"
```

`--status` must be one of `proposed`, `accepted`, `rejected`, `superseded`.

### `knogg proposal …`

Manage staged state-change proposals (created by the MCP
`propose_state_update` tool — see §6).

```bash
knogg proposal list             # PROP-0001  pending  state/active_context.yml
knogg proposal show PROP-0001   # full proposal: target, reason, patch, status
knogg proposal apply PROP-0001  # re-audit + apply, mark applied
knogg proposal reject PROP-0001 # mark rejected
```

- Only `pending` proposals can be applied or rejected.
- `apply` re-audits the patch (status must be valid) before applying.

### `knogg mcp`

Run the local MCP server (JSON-RPC over stdio). See §6.

### `knogg watch`

Watch `state/active_context.yml` and re-run `sync` on change (300–500 ms
debounce). Only `state/` is watched and `sync` writes elsewhere, so it cannot
loop. `core/` is never modified. Foreground process — Ctrl-C to stop.

```bash
knogg watch
```

---

## 6. MCP server (for AI agents)

`knogg mcp` speaks **JSON-RPC over stdio** (no HTTP/SSE). One JSON request per
line in, one JSON response per line out. The `method` is the tool name.

Run it (note `-T` for piped stdin under Docker):

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"<tool>","params":{…}}' \
  | docker compose run --rm -T dev cargo run -- mcp
# or:  printf … | ./knogg mcp
```

### Tools


| Tool                   | Params                    | Returns                                                                                                               |
| ---------------------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `tools/list`           | —                         | list of available tool names                                                                                          |
| `read_knogg`           | `{path}`                  | a knogg YAML file as JSON (path-boundary checked)                                                                     |
| `get_active_context`   | `{}`                      | resolved `state/active_context.yml` as JSON                                                                           |
| `list_knogg`           | `{include_proposals?}`    | safe relative paths; hides `backups/`, `.lock`, temp files; hides `state/proposals/` unless `include_proposals: true` |
| `get_tool_registry`    | `{}`                      | the registry mappings only — never template contents                                                                  |
| `propose_state_update` | `{target, patch, reason}` | creates a **pending** proposal `PROP-NNNN`; does **not** mutate state                                                 |
| `audit_commit`         | `{id}`                    | applies a staged proposal by id (re-audits first)                                                                     |


### Example requests

```bash
# read the active context
{"jsonrpc":"2.0","id":1,"method":"get_active_context","params":{}}

# list knogg files (without proposals)
{"jsonrpc":"2.0","id":1,"method":"list_knogg","params":{}}

# propose a state change (staged, not applied)
{"jsonrpc":"2.0","id":1,"method":"propose_state_update","params":{
  "target":"state/active_context.yml",
  "patch":{"focus":{"status":"in_progress"}},
  "reason":"Move to frontend UI work"}}

# apply a staged proposal
{"jsonrpc":"2.0","id":2,"method":"audit_commit","params":{"id":"PROP-0001"}}
```

Errors come back as JSON-RPC errors, e.g. an invalid `focus.status` or a
path-boundary violation.

### 6.4 Protocol compatibility

`knogg mcp` is a **minimal stdio JSON-RPC tool endpoint**. `method` is the tool
name directly (plus `tools/list`). It does **not** implement the MCP
`initialize` handshake or `tools/call` envelope.

- Works now: direct JSON-RPC pipes, scripts, custom agent code.
- Full MCP clients (Cursor / Claude / Codex auto-discovery) send `initialize`
  first — until that handshake lands, use the script bridge below.

### 6.5 Registering with an agent

Use an **absolute path** to the binary (agents do not run from the project
dir). Build it first: `make release` → `./dist/knogg`.

**Cursor** — `.cursor/mcp.json` (project) or `~/.cursor/mcp.json` (global):

```json
{
  "mcpServers": {
    "knogg": {
      "command": "/ABS/PATH/agknogg/dist/knogg",
      "args": ["mcp"]
    }
  }
}
```

**Claude Code** — project `.mcp.json`, or the CLI:

```bash
claude mcp add knogg -- /ABS/PATH/agknogg/dist/knogg mcp
```

```json
{
  "mcpServers": {
    "knogg": { "command": "/ABS/PATH/agknogg/dist/knogg", "args": ["mcp"] }
  }
}
```

**Claude Desktop** — `claude_desktop_config.json`
(macOS `~/Library/Application Support/Claude/`, Windows `%APPDATA%\Claude\`):

```json
{
  "mcpServers": {
    "knogg": { "command": "/ABS/PATH/agknogg/dist/knogg", "args": ["mcp"] }
  }
}
```

**Codex CLI** — `~/.codex/config.toml`:

```toml
[mcp_servers.knogg]
command = "/ABS/PATH/agknogg/dist/knogg"
args = ["mcp"]
```

**Other agents** — any MCP-capable agent: register a stdio server with
`command = <abs path>/dist/knogg` and `args = ["mcp"]`. Pass `--path` in
`args` if the knogg is not at `./.knogg` relative to the agent's cwd, e.g.
`["mcp", "--path", "/ABS/PATH/.knogg"]`.

Restart the agent after editing config. Confirm with a `tools/list` call.

---

## 7. Workflows

### 7.1 Human: start working on a task

```bash
knogg init                                            # once per project
knogg state set --stage auth --task "Add login" --status in_progress
knogg state add-next "Wire up session cookie"
knogg sync                                            # refresh tool config files
knogg handoff --to cursor --print                     # prompt to paste into Cursor
```

### 7.2 Human: record a decision

```bash
knogg decision add --title "Use JWT sessions" \
  --reason "Stateless, scales horizontally" --status accepted
```

### 7.3 AI agent: read context, propose a change

An agent connects over MCP and:

1. Calls `get_active_context` to learn the current focus.
2. Calls `list_knogg` / `read_knogg` for more detail if needed.
3. Calls `propose_state_update` — this **stages** `PROP-NNNN`, it does *not*
  change state.

The human then reviews and decides:

```bash
knogg proposal list
knogg proposal show PROP-0001
knogg proposal apply PROP-0001     # or: knogg proposal reject PROP-0001
knogg status                       # confirm the change landed
```

### 7.4 Reactive sync

```bash
# terminal 1
knogg watch
# terminal 2 — any state change triggers `sync` automatically
knogg state set --status done
```

---

## 8. Safety guarantees

- **Global lock** — every write acquires `.knogg/.lock` (RAII, released on
drop, 5 s timeout, clear error if held). No deadlocks, no infinite waits.
- **Atomic writes** — content is written to a temp file in the same directory,
flushed, then renamed over the target. A crash never leaves a partial file.
- **Backups** — `init --force` / `sync --force` back up every file they will
overwrite into `.knogg/backups/<timestamp>/` (only if content changes).
- **Staged proposals** — agents cannot mutate state directly; changes are
proposed and require explicit human `apply`.
- **Path boundaries** — `..` is always rejected; MCP also rejects absolute
paths and knogg escapes.
- **Human files respected** — `sync` never overwrites a file without the
generated-by marker unless `--force` is given.

---

## 9. Command summary


| Command                                                  | Purpose                                       |
| -------------------------------------------------------- | --------------------------------------------- |
| `knogg init [--force]`                                   | Create / regenerate the knogg tree            |
| `knogg status`                                           | Print project / stage / task / status         |
| `knogg doctor`                                           | Diagnose knogg integrity (exit ≠ 0 on error)  |
| `knogg handoff --to <agent> [--print] [--save <f>]`      | Render a handoff prompt                       |
| `knogg sync [--force] [--dry-run]`                       | Generate tool config files from templates     |
| `knogg state set [--stage] [--task] [--status]`          | Update the active context                     |
| `knogg state add-next "<text>"` / `clear-next`           | Manage next actions                           |
| `knogg decision add --title --reason --status [--scope]` | Append an ADR                                 |
| `knogg proposal list / show / apply / reject <id>`       | Manage staged proposals                       |
| `knogg mcp`                                              | Run the MCP server (JSON-RPC over stdio)      |
| `knogg watch`                                            | Re-run `sync` when the active context changes |


---

## 10. Known limitations

- **MCP transport is stdio only** — no HTTP / SSE / Streamable HTTP.
- **Clipboard is best-effort** — only when the `clipboard` feature is built;
otherwise (and inside Docker) `handoff` falls back to stdout.
- `**AGENTS.md` is the Codex / CLI-agent output** (not `.codex/context.md`);
the mapping lives in `plans/tool_registry.yml`.
- `[features]` / `[agents]` sections of `knogg.toml` are parsed but not yet
wired into behavior.
- Decisions live in a single `state/decision_log.yml` (no per-ADR files yet).

