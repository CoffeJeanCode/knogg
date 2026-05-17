<!-- generated-by: knogg -->
# Agent Guide — knogg

This project uses **knogg** to share working context between AI agents and
humans: what is being worked on, what was decided, what to do next.

## Brief

knogg is a small Rust CLI + MCP server. It keeps a per-project **context vault**
(`./.knogg/`) so multiple AI agents (Cursor, Claude, Codex, OpenCode) and humans
share one source of truth, without corrupting files or racing each other.

- **Single source of truth** — one `.knogg/` directory per project.
- **Safe writes** — every write takes a global lock + atomic rename.
- **Agent brokering** — handoff prompts + an MCP server over stdio.
- **Staged changes** — agents *propose*; humans *apply or reject*.
- **Agent Workspace Broker** — one registry renders MCP config for every agent.

Environment: Docker-compose only. Final binary is `knogg`.

## Structure

```
src/
  main.rs        # entrypoint, command dispatch
  cli.rs         # clap command definitions
  config.rs      # knogg.toml parsing + path precedence
  vault.rs       # init/status, active_context, AGENTS.md guide
  vaultio.rs     # global lock, atomic_write, backups, date helpers
  doctor.rs      # vault integrity diagnosis
  handoff.rs     # render handoff prompts (minijinja)
  sync.rs        # render tool config files from templates
  watch.rs       # reactive re-sync on state change (notify)
  state.rs       # `state set/add-next/clear-next`
  decision.rs    # ADR decision log
  proposal.rs    # staged state-change proposals
  messages.rs    # agent-to-agent message log
  mcp.rs         # MCP server (JSON-RPC over stdio)
  agents.rs      # Agent Workspace Broker (per-agent MCP configs)

.knogg/          # the vault (created by `knogg init`)
  core/          # stable project knowledge
  state/         # active_context, decision_log, proposals, messages
  plans/         # tool_registry, agent_registry, master_plan
  adapters/      # per-agent handoff templates
  backups/       # timestamped backups (on --force overwrite)
```

## MCP server

knogg exposes an MCP server over stdio (JSON-RPC: `initialize`,
`notifications/initialized`, `tools/list`, `tools/call`):

    knogg mcp

## Tools

### Core
- `get_active_context` — current project / stage / task / status / next actions.
- `get_brief` — compact project brief (next_actions, constraints, recent_decisions).
- `get_next_actions` — current next actions only.
- `get_allowed_scope` — constraints and scope an agent must respect.
- `get_current_decisions` — recent decisions from the brief.

### Vault I/O
- `read_vault {path}` — read one vault YAML file.
- `list_vault {include_proposals?}` — list safe vault file paths.
- `search_vault {query}` — case-insensitive text search across the vault.
- `get_tool_registry` — template -> output mappings.

### Proposals & Decisions
- `list_proposals` — staged proposals and their status.
- `propose_state_update {target, patch, reason}` — stage a state change.
- `audit_commit {id}` — apply a staged proposal.
- `propose_decision {title, reason, status?, scope?}` — record an ADR.

### Messages
- `post_message {from, text}` / `get_messages {limit?}` — agent message log.

### Roles & Agent Broker
- `list_roles` — list all defined agent roles.
- `get_role {name}` — get a role spec by name.
- `set_role {name, summary, responsibilities?, constraints?}` — create/replace a role.
- `get_agent_role {agent}` — get the role assigned to an agent.
- `set_agent_role {agent, role}` — assign a role to an agent.
- `get_agent_handoff {agent}` — render a handoff prompt for an agent.

## Workflow

1. Start every task by calling `get_active_context`.
2. Explore with `search_vault` / `read_vault` / `list_vault`.
3. To change state, NEVER write it directly — call `propose_state_update`.
   It stages a `PROP-NNNN` proposal (pending).
4. A human reviews and applies it (`knogg proposal apply <id>`).
5. Check `list_proposals` to see if your proposal was applied or rejected.
6. Record rationale with `propose_decision`.
7. Coordinate with other agents via `post_message` / `get_messages`.

## Standards

- Build and test only inside Docker: `docker compose run --rm dev cargo test`.
- Every file write goes through `vaultio::atomic_write` while holding a
  `VaultLock`. Never call `std::fs::write` directly.
- Reject `..` in any path; MCP also rejects absolute paths outside the vault.
- Add tests for every new behavior; `cargo test` must stay green, build warning-free.
- Match surrounding code style: English comments, terse, explain *why* not *what*.
- New dependencies need a real reason — prefer std and existing crates.

## Best practices

- Work in small, validated steps; run `cargo test` after each change.
- Read context first (`get_active_context`) before proposing anything.
- Keep proposals atomic and well-described (`reason` field is mandatory).
- Use `search_vault` before `read_vault` to locate the right file.
- Record non-obvious decisions with `propose_decision` so they survive handoffs.
- Run `knogg doctor` / `knogg agents doctor` after structural changes.
- Use `knogg sync --dry-run` and `knogg agents sync --dry-run` before writing.

## Don'ts

- Don't modify `docker-compose.yml`, `Dockerfile.dev`, or build the host env.
- Don't run a local `cargo` — there is no host Rust toolchain.
- Don't mutate `.knogg/state/` directly — agents propose, humans apply.
- Don't overwrite human-owned files; respect the generated-by marker / manifest.
- Don't overwrite files without `--force` (and `--force` always backs up first).
- Don't touch global agent configs — knogg is project-scoped only.
- Don't write secrets into versioned config files.
- Don't break or skip existing tests to land a change.

## Rules

- Agents propose; humans apply. No direct state mutation.
- Paths reject `..`; MCP also rejects absolute paths outside the vault.
- Valid `focus.status`: todo | in_progress | blocked | done.
