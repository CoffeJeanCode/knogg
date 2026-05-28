use clap::{Parser, Subcommand};

/// knogg — agent context broker.
#[derive(Parser, Debug)]
#[command(name = "knogg", version, about = "knogg — agent context broker", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize the vault local directory.
    Init {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        /// Overwrite existing files.
        #[arg(long)]
        force: bool,
        /// Also write an AGENTS.md guide for AI agents.
        #[arg(long)]
        agents_md: bool,
        /// Print a recommended prompt to give an AI agent for project setup.
        #[arg(long)]
        prompt: bool,
    },
    /// Generate a shell completion script (bash, zsh, fish, powershell, elvish).
    Completions {
        /// Target shell.
        shell: clap_complete::Shell,
    },
    /// Show the current vault status.
    Status {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
    },
    /// Diagnose the integrity of the vault.
    Doctor {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        /// Warn when staged proposals are still pending.
        #[arg(long, default_value_t = true)]
        pending_proposals: bool,
    },
    /// Generate a compact handoff prompt for an agent.
    Handoff {
        /// Target agent (cursor, claude, codex).
        #[arg(long)]
        to: String,
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        /// Print the prompt to stdout.
        #[arg(long)]
        print: bool,
        /// Write the prompt to this file (parent dirs are created).
        #[arg(long)]
        save: Option<String>,
        /// Auto-fill handoff.summary in active_context when empty.
        #[arg(long)]
        fill_summary: bool,
    },
    /// Agent message log (structured coordination).
    Messages {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: MessageAction,
    },
    /// Auto-configure the knogg MCP server for a supported IDE.
    Link {
        /// Target IDE: cursor or claude.
        ide: String,
    },
    /// Update the active context state.
    State {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: StateAction,
    },
    /// Manage the decision log.
    Decision {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: DecisionAction,
    },
    /// Manage staged state-change proposals.
    Proposal {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: ProposalAction,
    },
    /// Manage agent workspace configuration (MCP configs per agent).
    Agents {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: AgentsAction,
    },
    /// Manage agent role specifications.
    Role {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: RoleAction,
    },
    /// Manage event hooks.
    Hooks {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: HooksAction,
    },
    /// Manage the compact project brief.
    Brief {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: BriefAction,
    },
    /// Run the local MCP server over stdio.
    Mcp {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
    },
    /// Watch the vault and synchronize reactively.
    Watch {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
    },
    /// Partitioned tasks in plans/master_plan.yml.
    Task {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Start a P2P serve daemon — TCP JSON-RPC (read-only) on a port.
    Serve {
        /// TCP port to listen on.
        #[arg(long, default_value_t = 5051)]
        port: u16,
    },
    /// Clear vault lock files manually.
    Unlock {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        /// Clear every `.lock` file under the vault.
        #[arg(long)]
        all: bool,
        /// Clear the lock for one vault-relative file (e.g. state/active_context.yml).
        #[arg(long)]
        file: Option<String>,
    },
    /// Reclaim disk space: purge old backups + terminal proposals.
    Gc {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        /// Show what would be deleted without removing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Self-update: check GitHub releases and upgrade to the latest version.
    Update {
        /// Only check for a newer version and report it; do not download.
        #[arg(long)]
        check: bool,
    },
    /// Coding conventions from `core/style_guides.yml`.
    Style {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        #[command(subcommand)]
        action: StyleAction,
    },
    /// Read and write knogg.yml project configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Interactively approve or reject pending proposals.
    Triage {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum TaskAction {
    /// List structured tasks in the master plan.
    List,
    /// Claim a task (status → in_progress).
    Claim {
        /// Task id, e.g. 7A.
        id: String,
        /// Agent claiming the task.
        #[arg(long)]
        agent: String,
    },
    /// Release a task (status → done).
    Release {
        /// Task id.
        id: String,
        /// Agent releasing the task.
        #[arg(long)]
        agent: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum StyleAction {
    /// List languages with style guides in the vault.
    List,
    /// Show rules for one language.
    Show {
        /// Language id (e.g. rust).
        #[arg(long)]
        lang: String,
    },
    /// Check conventions (module docs, optional rustfmt).
    Doctor {
        /// Run `cargo fmt --check` when Cargo.toml is present.
        #[arg(long)]
        check_fmt: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum StateAction {
    /// Set stage, task, and/or status.
    Set {
        /// New stage.
        #[arg(long)]
        stage: Option<String>,
        /// New task.
        #[arg(long)]
        task: Option<String>,
        /// New status (todo, in_progress, blocked, done).
        #[arg(long)]
        status: Option<String>,
    },
    /// Append a next action.
    AddNext {
        /// The next-action text.
        action: String,
    },
    /// Remove all next actions.
    ClearNext,
}

#[derive(Subcommand, Debug)]
pub enum ProposalAction {
    /// List all proposals.
    List,
    /// Show one or more proposals by id (read-only).
    Show {
        /// Proposal ids, e.g. PROP-0001 PROP-0002.
        #[arg(required = true, num_args = 1..)]
        ids: Vec<String>,
    },
    /// Apply pending proposal(s). Best-effort batch — not atomic; each id is independent.
    Apply {
        /// Proposal ids, e.g. PROP-0001 PROP-0002.
        #[arg(required = true, num_args = 1..)]
        ids: Vec<String>,
    },
    /// Reject pending proposal(s). Best-effort batch — not atomic.
    Reject {
        /// Proposal ids, e.g. PROP-0001 PROP-0002.
        #[arg(required = true, num_args = 1..)]
        ids: Vec<String>,
    },
    /// Remove terminal proposals (applied/rejected).
    Gc {
        /// Statuses to reap (repeatable; default: applied, rejected).
        #[arg(long = "status")]
        statuses: Vec<String>,
        /// Keep at most N files per status (oldest removed first).
        #[arg(long)]
        keep: Option<usize>,
        /// Only GC proposals tagged with this project name.
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum MessageAction {
    /// List messages with optional filters.
    List {
        /// Filter by sender.
        #[arg(long)]
        from: Option<String>,
        /// Filter by recipient (includes broadcast messages).
        #[arg(long)]
        to: Option<String>,
        /// Filter by status (open, acked, closed).
        #[arg(long)]
        status: Option<String>,
        /// Messages not yet read by this agent.
        #[arg(long)]
        unread_for: Option<String>,
        /// Maximum number of messages (newest last).
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Mark message(s) read/acked by an agent. Best-effort batch — not atomic.
    Ack {
        /// Message ids, e.g. MSG-0001 MSG-0002.
        #[arg(required = true, num_args = 1..)]
        ids: Vec<String>,
        /// Agent acknowledging the message(s).
        #[arg(long)]
        by: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentsAction {
    /// List agents in the registry.
    List,
    /// Validate the agent registry.
    Doctor,
    /// Inspect existing project agent configs.
    Inspect,
    /// Diff existing configs against the registry.
    Diff,
    /// Import MCP servers from an existing agent config into the registry.
    Generalize {
        /// Source agent (cursor, claude, codex, opencode).
        #[arg(long)]
        from: String,
        /// Overwrite registry entries that already exist.
        #[arg(long)]
        force: bool,
    },
    /// Render and write per-agent MCP config files.
    Sync {
        /// Show what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Overwrite human-owned files.
        #[arg(long)]
        force: bool,
    },
    /// Assign a role to an agent.
    SetRole {
        /// Agent name.
        agent: String,
        /// Role name (must exist in plans/roles.yml).
        role: String,
    },
    /// Enable an agent.
    Enable {
        /// Agent name.
        agent: String,
    },
    /// Disable an agent.
    Disable {
        /// Agent name.
        agent: String,
    },
    /// Attach an MCP server to an agent.
    EnableMcp {
        /// Agent name.
        agent: String,
        /// MCP server name.
        server: String,
    },
    /// Detach an MCP server from an agent.
    DisableMcp {
        /// Agent name.
        agent: String,
        /// MCP server name.
        server: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum HooksAction {
    /// List hooks.
    List,
    /// Validate hooks (unknown events / actions).
    Doctor,
    /// Run the actions for an event.
    Run {
        /// Event name.
        event: String,
    },
    /// Enable an event's hooks.
    Enable {
        /// Event name.
        event: String,
    },
    /// Disable an event's hooks.
    Disable {
        /// Event name.
        event: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum BriefAction {
    /// Regenerate the brief.
    Refresh,
    /// Print the brief.
    Show,
    /// Validate the brief.
    Doctor,
}

#[derive(Subcommand, Debug)]
pub enum RoleAction {
    /// Create or replace a role.
    Set {
        /// Role name.
        name: String,
        /// What the agent in this role is.
        #[arg(long)]
        summary: String,
        /// A responsibility (repeatable).
        #[arg(long = "responsibility")]
        responsibilities: Vec<String>,
        /// A constraint (repeatable).
        #[arg(long = "constraint")]
        constraints: Vec<String>,
    },
    /// List all roles.
    List,
    /// Show a role by name.
    Show {
        /// Role name.
        name: String,
    },
    /// Remove a role by name.
    Remove {
        /// Role name.
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum DecisionAction {
    /// Append a new ADR entry.
    Add {
        /// Decision title.
        #[arg(long)]
        title: String,
        /// Why the decision was made.
        #[arg(long)]
        reason: String,
        /// Status (proposed, accepted, rejected, superseded).
        #[arg(long)]
        status: String,
        /// Scope of the decision.
        #[arg(long, default_value = "global")]
        scope: String,
    },
    /// Update status on existing ADR(s). Best-effort batch — not atomic.
    SetStatus {
        /// ADR ids, e.g. ADR-0005 ADR-0006.
        #[arg(required = true, num_args = 1..)]
        ids: Vec<String>,
        /// New status (proposed, accepted, rejected, superseded).
        #[arg(long)]
        status: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print knogg.yml (or legacy knogg.toml).
    Show,
    /// Set a config value using dot-notation key (e.g. mesh.listen_port 5051).
    Set {
        /// Dot-notation key, e.g. mesh.listen_port or mesh.peers.backend.
        key: String,
        /// Value to set (auto-typed: bool, integer, or string).
        value: String,
    },
    /// Get a config value by dot-notation key.
    Get {
        /// Dot-notation key.
        key: String,
    },
}
