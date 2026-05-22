# Configuration

## `knogg.toml`

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

[mesh]
listen_port = 5051

[mesh.peers]
backend = "tcp://localhost:5052"
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
│   │   └── mod.rs           # JSON-RPC stdio server
│   ├── commands/
│   │   ├── agents.rs        # Agent registry, sync, inspect, enable/disable
│   │   ├── brief.rs         # Brief refresh, show, doctor
│   │   ├── decision.rs      # ADR log management
│   │   ├── doctor.rs        # Integrity diagnostics
│   │   ├── handoff.rs       # Handoff prompt rendering
│   │   ├── hooks.rs         # Event-driven hook execution
│   │   ├── messages.rs      # Agent message log
│   │   ├── plan.rs          # Task claim/release
│   │   ├── proposal.rs      # Stage/apply/reject/gc proposals
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
├── .knogg/                  # Vault (see vault.md)
├── .github/                 # CI, issue/PR templates
├── scripts/
│   ├── install.sh           # Linux/macOS installer
│   └── install.ps1          # Windows installer
├── Cargo.toml
├── docker-compose.yml
├── Dockerfile.dev
├── Makefile
├── LICENSE
├── CHANGELOG.md
├── CONTRIBUTING.md
└── SECURITY.md
```

---

## Known Limitations

- **MCP transport is stdio only** — no HTTP / SSE / Streamable HTTP
- **Clipboard is best-effort** — only when `clipboard` feature is built; otherwise `handoff` falls back to stdout
- **`[features]` / `[agents]` sections** of `knogg.toml` parsed but not yet wired into behavior
- **Decisions** live in single `state/decision_log.yml` (no per-ADR files)
- **Lock timeout** (5s) may be insufficient for very large vaults under heavy concurrent access
