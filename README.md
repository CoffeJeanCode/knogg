# knogg

**Agent context broker** — a small Rust CLI that keeps a local context store for AI coding agents and brokers that context between tools (Cursor, Claude Code, Codex). Multiple agents and humans share one source of truth for *what is being worked on*, *what was decided*, and *what to do next* — without corrupting files or stepping on each other.

[![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-APACHE2.0-blue)](LICENSE)
[![Version](https://img.shields.io/github/v/release/CoffeJeanCode/knogg?label=version)](https://github.com/CoffeJeanCode/knogg/releases)
[![Docker](https://img.shields.io/badge/Docker-first-2496ED?logo=docker)](docker-compose.yml)
[![MCP](https://img.shields.io/badge/MCP-stdio-black)](docs/mcp.md)
[![Mesh](https://img.shields.io/badge/Mesh-federation-7B61FF)](docs/mesh.md)

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
| **Mesh federation** | Cross-project agent communication via TCP hub (`knogg hub`) |
| **P2P peering** | Direct peer-to-peer connections via `knogg.toml [mesh.peers]` |
| **Event subscriptions** | Cross-repo state change propagation |
| **Stale lock reclamation** | Dead-PID detection + auto-reclaim after 30s |
| **Schema migrations** | Transparent vault YAML upgrades on read |

## Quick Start

```bash
# Install (Linux/macOS)
curl -fsSL https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.sh | bash

# Install (Windows PowerShell)
irm https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.ps1 | iex

# Initialize and use
knogg init
knogg state set --stage auth --task "Add login" --status in_progress
knogg sync
knogg handoff --to cursor --print
```

## Documentation

| Doc | Contents |
|-----|----------|
| [Installation](#installation) | Installers, binaries, build from source |
| [docs/commands.md](docs/commands.md) | Full command reference |
| [docs/mcp.md](docs/mcp.md) | MCP tools, registering with agents |
| [docs/mesh.md](docs/mesh.md) | Hub federation + P2P peering |
| [docs/vault.md](docs/vault.md) | Vault layout, safety guarantees, maintenance |
| [docs/configuration.md](docs/configuration.md) | knogg.toml, path precedence, project structure |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Dev setup, code style, PR process |
| [CHANGELOG.md](CHANGELOG.md) | Release history |

---

## Installation

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.sh | bash
```

Custom install directory:

```bash
KNOGG_INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.ps1 | iex
```

Custom directory or pinned version:

```powershell
$env:KNOGG_INSTALL_DIR = "C:\Tools\knogg"; irm https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.ps1 | iex
$env:KNOGG_VERSION = "v1.1.0";             irm https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.ps1 | iex
```

### Download Binary

Pre-built binaries at **https://github.com/CoffeJeanCode/knogg/releases**:

| Platform | Asset |
|----------|-------|
| Linux x86_64 | `knogg-linux-amd64` |
| macOS Intel | `knogg-macos-amd64` |
| macOS Apple Silicon | `knogg-macos-arm64` |
| Windows x86_64 | `knogg-windows-amd64.exe` |

```bash
# Linux / macOS
curl -LO https://github.com/CoffeJeanCode/knogg/releases/latest/download/knogg-linux-amd64
chmod +x knogg-linux-amd64
sudo mv knogg-linux-amd64 /usr/local/bin/knogg
```

### Build from Source

Requires Docker (Compose v2) — no local Rust toolchain needed.

```bash
make release    # → ./dist/knogg + ./dist/knogg.exe
make test
make dev        # interactive dev shell
```

| Method | Command | When |
|--------|---------|------|
| Wrapper | `./knogg <cmd>` | Daily use — uses `./dist/knogg` if built, else Docker |
| Binary | `./dist/knogg <cmd>` | Fastest — requires `make release` |
| Docker | `docker compose run --rm dev cargo run -- <cmd>` | CI / no binary |

---

## Typical Workflow

```bash
# Human: set focus
knogg state set --stage auth --task "Add login" --status in_progress
knogg state add-next "Wire up session cookie"
knogg sync
knogg handoff --to cursor --print

# Human: record a decision
knogg decision add --title "Use JWT sessions" \
  --reason "Stateless, scales horizontally" --status accepted

# AI agent: read context → propose change (via MCP)
# get_active_context → propose_state_update → PROP-NNNN staged

# Human: review + apply
knogg proposal show PROP-0001
knogg proposal apply PROP-0001
knogg status
```

---

## License

Apache License 2.0 — see [LICENSE](LICENSE).
