# Knogg Mesh

Two federation modes: **hub** (central router) and **P2P** (direct peering). Both let agents in one project read context from another.

---

## Hub Mode

Central TCP router. All projects connect to it; agents query other projects through it.

```
Project A ──┐
            ├──→ Knogg Hub (TCP :5050) ←── Project B
Project C ──┘
```

### Start the Hub

```bash
knogg hub                  # port 5050
knogg hub --port 6060
```

Via Docker:

```bash
docker compose run --rm -p 5050:5050 dev cargo run -- hub --port 5050
```

### Connect a Project

```bash
export KNOGG_HUB_URL="tcp://localhost:5050"
export KNOGG_PROJECT="my-project"   # optional — defaults to directory name
knogg status                        # auto-connects on first command
```

### Query Another Project (MCP)

```json
{
  "method": "query_mesh",
  "params": {
    "target_project": "other-project",
    "query": "get_active_context",
    "args": {}
  }
}
```

Supported queries: `get_active_context`, `read_vault`, `list_vault`, `search_vault`.

### List Connected Peers

```bash
knogg mesh list-peers
```

---

## P2P Mode

Direct TCP connections between vaults. No hub needed.

```
knogg serve :5051 ←→ knogg serve :5052
   (frontend)              (backend)
       │                       │
    .knogg/                 .knogg/
```

Each vault serves read-only JSON-RPC. Peers auto-reconnect on failure.

### Configuration

Add to `knogg.toml`:

```toml
[mesh]
listen_port = 5051

[mesh.peers]
backend = "tcp://localhost:5052"
db      = "tcp://localhost:5053"
```

### Start P2P Nodes

```bash
# Terminal 1
knogg serve --port 5051

# Terminal 2
cd /path/to/other-project
knogg serve --port 5052
```

### Query a Peer (MCP)

```json
{
  "method": "query_peer",
  "params": {
    "peer": "backend",
    "method": "get_active_context",
    "params": {}
  }
}
```

### Subscribe to Task Events

```json
{
  "method": "subscribe_to_task",
  "params": {
    "peer": "backend",
    "task_id": "API-Auth",
    "from": "frontend-agent"
  }
}
```

When the peer marks that task done via `knogg state set --status done`, subscribers receive a `task_done` event and trigger automatic sync.

---

## Use Cases

- **Multi-repo** — agents across frontend/backend repos share context
- **Monorepo sub-projects** — each package has its own `.knogg/` but agents see the full picture
- **Cross-team visibility** — team A checks team B's active tasks without leaving their project
