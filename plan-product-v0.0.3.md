Objetivo de la nueva fase

Agregar a Knogg dos capacidades:

1. Hooks operativos
   Automatizar acciones alrededor de eventos: init, sync, handoff, proposal, MCP call, state change.

2. Context minimization
   Reducir tokens y deliberación innecesaria entregando contexto ya resumido, estructurado y filtrado.
Problema actual

Aunque Knogg ya centraliza contexto, todavía puede pasar esto:

- El agente lee más contexto del necesario.
- Cada agente vuelve a inferir decisiones ya tomadas.
- MCP devuelve datos demasiado genéricos o extensos.
- No hay hooks para preparar/resumir/validar contexto antes de cada handoff.
- No hay una capa clara de “qué necesita saber este agente ahora”.
Nueva fase propuesta: F — Hooks & Context Minimization
F1 — knogg hooks

Agregar sistema de hooks configurable.

Archivo nuevo
.ai-vault/hooks.yml

Ejemplo:

version: 1

hooks:
  before_handoff:
    enabled: true
    actions:
      - "refresh_brief"
      - "validate_active_context"

  after_state_change:
    enabled: true
    actions:
      - "sync"
      - "refresh_brief"

  before_mcp_response:
    enabled: true
    actions:
      - "compress_context"

  after_proposal_created:
    enabled: true
    actions:
      - "append_agent_note"
Comandos
knogg hooks list
knogg hooks doctor
knogg hooks run before_handoff
knogg hooks enable before_handoff
knogg hooks disable before_handoff
Uso
knogg hooks run before_handoff
knogg handoff --to cursor --save .handoff/cursor.md
F2 — Brief canónico para agentes

Crear un archivo derivado, pequeño y optimizado para consumo de agentes.

Archivo nuevo
.ai-vault/state/brief.yml

Ejemplo:

version: 1

current:
  stage: "frontend-ui"
  task: "Implement subscription badge"
  status: "in_progress"

allowed_scope:
  - "apps/web/**"
  - "packages/ui/**"
  - "packages/contracts/**"

forbidden_scope:
  - "services/**"

known_facts:
  - "Backend already exposes subscription_status"
  - "Contract package has been updated"
  - "UI must support active, past_due, canceled"

decisions:
  - "Use AGENTS.md for Codex/CLI agents"
  - "Agents propose changes through staged proposals"

next_actions:
  - "Update currentUser query"
  - "Render SubscriptionStatusBadge"
  - "Add UI test"

agent_instruction:
  mode: "frontend_only"
  avoid:
    - "Do not modify backend services"
    - "Do not change MCP configuration"

Este brief.yml sería la fuente principal para:

- handoff
- MCP get_active_context
- Cursor rules
- Claude context
- AGENTS.md
F3 — knogg brief refresh

Agregar comando para regenerar el brief desde el estado completo.

knogg brief refresh
knogg brief show
knogg brief doctor
Entrada

Lee:

.ai-vault/state/active_context.yml
.ai-vault/state/decision_log.yml
.ai-vault/plans/tool_registry.yml
.ai-vault/plans/agent_registry.yml
.ai-vault/state/proposals/
Salida

Escribe:

.ai-vault/state/brief.yml
Regla importante

No copiar todo. Solo incluir:

- estado actual
- decisiones relevantes
- próximos pasos
- scopes permitidos/prohibidos
- riesgos actuales
- último handoff útil
F4 — MCP tools más pequeñas y específicas

En vez de que los agentes llamen tools genéricas, agregar tools semánticas:

get_brief
get_next_actions
get_allowed_scope
get_current_decisions
get_agent_handoff
propose_next_state
Ejemplo MCP
get_brief
{
  "method": "tools/call",
  "params": {
    "name": "get_brief",
    "arguments": {}
  }
}

Devuelve solo:

{
  "stage": "frontend-ui",
  "task": "Implement subscription badge",
  "scope": {
    "allowed": ["apps/web/**", "packages/ui/**"],
    "forbidden": ["services/**"]
  },
  "next_actions": [
    "Update currentUser query",
    "Render SubscriptionStatusBadge"
  ]
}

Esto reduce mucho la necesidad de que el agente pregunte o razone sobre el estado global.

F5 — Context profiles por agente

Cada agente no necesita el mismo contexto.

Agregar en:

.ai-vault/plans/agent_registry.yml

Ejemplo:

agents:
  cursor:
    enabled: true
    context_profile: "frontend_executor"
    include:
      - "brief.current"
      - "brief.allowed_scope"
      - "brief.next_actions"
    exclude:
      - "decision_history"
      - "backend_details"

  claude:
    enabled: true
    context_profile: "planner_or_backend"
    include:
      - "brief.current"
      - "brief.known_facts"
      - "brief.decisions"
      - "brief.next_actions"

  codex:
    enabled: true
    context_profile: "repo_executor"
    include:
      - "brief.current"
      - "brief.allowed_scope"
      - "brief.forbidden_scope"
      - "brief.agent_instruction"

  opencode:
    enabled: true
    context_profile: "fast_patch_agent"
    include:
      - "brief.current"
      - "brief.next_actions"

Así knogg handoff --to cursor no entrega lo mismo que knogg handoff --to claude.

F6 — Hooks automáticos integrados

Integrar hooks en comandos existentes.

handoff

Antes:

knogg handoff --to cursor

Después:

before_handoff:
  - validate_active_context
  - refresh_brief
  - render_agent_profile
sync
before_sync:
  - validate_registry
  - refresh_brief

after_sync:
  - doctor_generated_outputs
proposal apply
before_proposal_apply:
  - audit_commit
  - backup_state

after_proposal_apply:
  - refresh_brief
  - sync
mcp get_brief
before_mcp_response:
  - ensure_brief_fresh
  - compress_context
Fase G — Communication Layer entre agentes

Esto ya apunta directo a reducir comunicación repetida.

G1 — Agent notes

Agregar:

.ai-vault/state/agent_notes.yml

Ejemplo:

version: 1

notes:
  - id: "NOTE-0001"
    agent: "claude"
    stage: "backend-contract"
    summary: "Backend contract is complete"
    changed:
      - "packages/contracts/user.ts"
      - "services/billing/subscription.rs"
    handoff_to: "cursor"
    next:
      - "Update UI query"
      - "Render subscription badge"

Comando:

knogg note add \
  --agent claude \
  --summary "Backend contract is complete" \
  --handoff-to cursor
G2 — knogg handoff from-note
knogg handoff --from-note NOTE-0001 --to cursor

Esto genera un handoff pequeño y específico basado en el output real del agente anterior.

Fase H — Decision packets

Para reducir CoT, dale al agente “paquetes de decisión” ya preparados.

Archivo
.ai-vault/state/decision_packets.yml

Ejemplo:

version: 1

packets:
  - id: "PKT-frontend-subscription-ui"
    applies_to:
      - "cursor"
      - "codex"
    problem: "Show subscription status in billing settings"
    decision: "Use shared contract field subscription_status"
    rationale: "Avoid duplicating billing logic in frontend"
    constraints:
      - "Do not call billing service directly from frontend"
      - "Use currentUser query"
    next_actions:
      - "Update query"
      - "Render badge"
      - "Add UI test"

MCP tool:

get_decision_packet

Así el agente recibe:

Problema → decisión → razón → constraints → acciones

en vez de inferirlo.

Orden recomendado de implementación
F1 hooks.yml + hooks list/doctor/run
F2 brief.yml schema
F3 brief refresh/show/doctor
F4 MCP get_brief/get_next_actions/get_allowed_scope
F5 context profiles por agente
F6 hooks integrados en handoff/sync/proposal
G1 agent_notes.yml + note add/list/show
G2 handoff --from-note
H1 decision_packets.yml
H2 MCP get_decision_packet