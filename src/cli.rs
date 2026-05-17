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
    },
    /// Generate tool-specific config files from templates.
    Sync {
        /// Vault path (overrides knogg.toml; defaults to ./.knogg).
        #[arg(long)]
        path: Option<String>,
        /// Overwrite human-owned files.
        #[arg(long)]
        force: bool,
        /// Show what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
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
    /// Show a proposal by id.
    Show {
        /// Proposal id, e.g. PROP-0001.
        id: String,
    },
    /// Apply a pending proposal.
    Apply {
        /// Proposal id, e.g. PROP-0001.
        id: String,
    },
    /// Reject a pending proposal.
    Reject {
        /// Proposal id, e.g. PROP-0001.
        id: String,
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
}
