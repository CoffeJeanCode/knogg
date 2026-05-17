mod agents;
mod brief;
mod cli;
mod config;
mod hooks;
mod decision;
mod doctor;
mod handoff;
mod mcp;
mod messages;
mod proposal;
mod roles;
mod state;
mod sync;
mod vault;
mod vaultio;
mod watch;

use clap::Parser;
use cli::{Cli, Commands, DecisionAction, ProposalAction, StateAction};
use cli::{AgentsAction, BriefAction, HooksAction, RoleAction};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = config::load()?;

    // CLI --path > knogg.toml [knogg].path > default ./.knogg.
    let resolve = |path| config::resolve_vault_path(path, &cfg);
    let marker = cfg.generated_marker();

    match cli.command {
        Commands::Init { path, force, agents_md } => {
            vault::init(&resolve(path), force)?;
            if agents_md {
                vault::write_agents_md(force)?;
            }
        }
        Commands::Status { path } => {
            vault::status(&resolve(path))?;
        }
        Commands::Doctor { path } => {
            doctor::doctor(&resolve(path), &marker)?;
        }
        Commands::Handoff { to, path, print, save } => {
            handoff::handoff(&to, &resolve(path), print, save.as_deref())?;
        }
        Commands::Sync { path, force, dry_run } => {
            sync::sync(&resolve(path), force, &marker, dry_run)?;
        }
        Commands::State { path, action } => {
            let p = resolve(path);
            match action {
                StateAction::Set { stage, task, status } => {
                    state::set(&p, stage, task, status)?;
                }
                StateAction::AddNext { action } => {
                    state::add_next(&p, &action)?;
                }
                StateAction::ClearNext => {
                    state::clear_next(&p)?;
                }
            }
        }
        Commands::Decision { path, action } => {
            let p = resolve(path);
            match action {
                DecisionAction::Add { title, reason, status, scope } => {
                    decision::add(&p, &title, &reason, &status, &scope)?;
                }
            }
        }
        Commands::Proposal { path, action } => {
            let p = resolve(path);
            match action {
                ProposalAction::List => proposal::cmd_list(&p)?,
                ProposalAction::Show { id } => proposal::cmd_show(&p, &id)?,
                ProposalAction::Apply { id } => proposal::cmd_apply(&p, &id)?,
                ProposalAction::Reject { id } => proposal::cmd_reject(&p, &id)?,
            }
        }
        Commands::Agents { path, action } => {
            let p = resolve(path);
            match action {
                AgentsAction::List => agents::cmd_list(&p)?,
                AgentsAction::Doctor => agents::cmd_doctor(&p)?,
                AgentsAction::Inspect => agents::cmd_inspect(&p)?,
                AgentsAction::Diff => agents::cmd_diff(&p)?,
                AgentsAction::Generalize { from, force } => {
                    agents::cmd_generalize(&p, &from, force)?;
                }
                AgentsAction::Sync { dry_run, force } => {
                    agents::sync(&p, force, dry_run)?;
                }
                AgentsAction::SetRole { agent, role } => {
                    agents::cmd_set_role(&p, &agent, &role)?;
                }
                AgentsAction::Enable { agent } => {
                    agents::set_agent_enabled(&p, &agent, true)?;
                }
                AgentsAction::Disable { agent } => {
                    agents::set_agent_enabled(&p, &agent, false)?;
                }
                AgentsAction::EnableMcp { agent, server } => {
                    agents::set_agent_mcp(&p, &agent, &server, true)?;
                }
                AgentsAction::DisableMcp { agent, server } => {
                    agents::set_agent_mcp(&p, &agent, &server, false)?;
                }
            }
        }
        Commands::Role { path, action } => {
            let p = resolve(path);
            match action {
                RoleAction::Set { name, summary, responsibilities, constraints } => {
                    roles::cmd_set(&p, &name, &summary, responsibilities, constraints)?;
                }
                RoleAction::List => roles::cmd_list(&p)?,
                RoleAction::Show { name } => roles::cmd_show(&p, &name)?,
                RoleAction::Remove { name } => roles::cmd_remove(&p, &name)?,
            }
        }
        Commands::Hooks { path, action } => {
            let p = resolve(path);
            match action {
                HooksAction::List => hooks::cmd_list(&p)?,
                HooksAction::Doctor => hooks::cmd_doctor(&p)?,
                HooksAction::Run { event } => hooks::cmd_run(&p, &event)?,
                HooksAction::Enable { event } => hooks::cmd_set_enabled(&p, &event, true)?,
                HooksAction::Disable { event } => hooks::cmd_set_enabled(&p, &event, false)?,
            }
        }
        Commands::Brief { path, action } => {
            let p = resolve(path);
            match action {
                BriefAction::Refresh => brief::cmd_refresh(&p)?,
                BriefAction::Show => brief::cmd_show(&p)?,
                BriefAction::Doctor => brief::cmd_doctor(&p)?,
            }
        }
        Commands::Mcp { path } => {
            mcp::serve(&resolve(path))?;
        }
        Commands::Watch { path } => {
            watch::watch(&resolve(path), &marker)?;
        }
    }

    Ok(())
}
