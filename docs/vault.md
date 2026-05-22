# Vault

## Layout

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

---

## Active Context

`state/active_context.yml` is the single source of truth for current focus:

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
handoff:
  summary: ""
```

Edit via `knogg state set …` — never hand-edit (bypasses lock + atomic write).

---

## Safety Guarantees

| Guarantee | Implementation |
|-----------|----------------|
| **Global lock** | `.knogg/.lock` — RAII, released on drop, 15s timeout, stale-lock auto-reclaim |
| **Atomic writes** | Temp file + rename — crash never leaves partial file |
| **Backups** | `init --force` / `sync --force` back up changed files to `.knogg/backups/<timestamp>/` |
| **Staged proposals** | Agents cannot mutate state directly; changes require human `apply` |
| **Path boundaries** | `..` always rejected; MCP rejects absolute paths and vault escapes |
| **Human files respected** | `sync` never overwrites without the generated-by marker unless `--force` |
| **Stale lock reclamation** | Locks with dead PIDs reclaimed after 30s (`kill(pid, 0)` liveness check) |
| **Schema migrations** | Transparent vault YAML upgrades on read |

---

## Maintenance

### Unlock stale locks

```bash
knogg unlock --all
knogg unlock --file state/active_context.yml
```

Locks auto-reclaim after 30s — manual unlock only needed in edge cases.

### Reclaim disk space

```bash
knogg gc                # dry-run by default
knogg gc --dry-run      # explicit preview
```

Removes:
- `.knogg/backups/<stamp>/` older than 7 days
- `state/proposals/*.yml` with `applied` or `rejected` status
