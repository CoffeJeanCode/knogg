mod cli;
mod commands;
mod core;
mod mcp;

use clap::Parser;
use cli::{Cli, Commands, DecisionAction, ProposalAction, StateAction};
use cli::{AgentsAction, BriefAction, HooksAction, RoleAction};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = core::config::load()?;

    // CLI --path > knogg.toml [knogg].path > default ./.knogg.
    let resolve = |path| core::config::resolve_vault_path(path, &cfg);
    let marker = cfg.generated_marker();

    match cli.command {
        Commands::Init { path, force, agents_md, prompt } => {
            if prompt {
                core::vault::print_setup_prompt();
                return Ok(());
            }
            core::vault::init(&resolve(path), force)?;
            if agents_md {
                core::vault::write_agents_md(force)?;
            }
        }
        Commands::Status { path } => {
            core::vault::status(&resolve(path))?;
        }
        Commands::Doctor { path } => {
            commands::doctor::doctor(&resolve(path), &marker)?;
        }
        Commands::Handoff { to, path, print, save } => {
            commands::handoff::handoff(&to, &resolve(path), print, save.as_deref())?;
        }
        Commands::Sync { path, force, dry_run } => {
            commands::sync::sync(&resolve(path), force, &marker, dry_run)?;
        }
        Commands::State { path, action } => {
            let p = resolve(path);
            match action {
                StateAction::Set { stage, task, status } => {
                    commands::state::set(&p, stage, task, status)?;
                }
                StateAction::AddNext { action } => {
                    commands::state::add_next(&p, &action)?;
                }
                StateAction::ClearNext => {
                    commands::state::clear_next(&p)?;
                }
            }
        }
        Commands::Decision { path, action } => {
            let p = resolve(path);
            match action {
                DecisionAction::Add { title, reason, status, scope } => {
                    commands::decision::add(&p, &title, &reason, &status, &scope)?;
                }
            }
        }
        Commands::Proposal { path, action } => {
            let p = resolve(path);
            match action {
                ProposalAction::List => commands::proposal::cmd_list(&p)?,
                ProposalAction::Show { id } => commands::proposal::cmd_show(&p, &id)?,
                ProposalAction::Apply { id } => commands::proposal::cmd_apply(&p, &id)?,
                ProposalAction::Reject { id } => commands::proposal::cmd_reject(&p, &id)?,
            }
        }
        Commands::Agents { path, action } => {
            let p = resolve(path);
            match action {
                AgentsAction::List => commands::agents::cmd_list(&p)?,
                AgentsAction::Doctor => commands::agents::cmd_doctor(&p)?,
                AgentsAction::Inspect => commands::agents::cmd_inspect(&p)?,
                AgentsAction::Diff => commands::agents::cmd_diff(&p)?,
                AgentsAction::Generalize { from, force } => {
                    commands::agents::cmd_generalize(&p, &from, force)?;
                }
                AgentsAction::Sync { dry_run, force } => {
                    commands::agents::sync(&p, force, dry_run)?;
                }
                AgentsAction::SetRole { agent, role } => {
                    commands::agents::cmd_set_role(&p, &agent, &role)?;
                }
                AgentsAction::Enable { agent } => {
                    commands::agents::set_agent_enabled(&p, &agent, true)?;
                }
                AgentsAction::Disable { agent } => {
                    commands::agents::set_agent_enabled(&p, &agent, false)?;
                }
                AgentsAction::EnableMcp { agent, server } => {
                    commands::agents::set_agent_mcp(&p, &agent, &server, true)?;
                }
                AgentsAction::DisableMcp { agent, server } => {
                    commands::agents::set_agent_mcp(&p, &agent, &server, false)?;
                }
            }
        }
        Commands::Role { path, action } => {
            let p = resolve(path);
            match action {
                RoleAction::Set { name, summary, responsibilities, constraints } => {
                    commands::roles::cmd_set(&p, &name, &summary, responsibilities, constraints)?;
                }
                RoleAction::List => commands::roles::cmd_list(&p)?,
                RoleAction::Show { name } => commands::roles::cmd_show(&p, &name)?,
                RoleAction::Remove { name } => commands::roles::cmd_remove(&p, &name)?,
            }
        }
        Commands::Hooks { path, action } => {
            let p = resolve(path);
            match action {
                HooksAction::List => commands::hooks::cmd_list(&p)?,
                HooksAction::Doctor => commands::hooks::cmd_doctor(&p)?,
                HooksAction::Run { event } => commands::hooks::cmd_run(&p, &event)?,
                HooksAction::Enable { event } => commands::hooks::cmd_set_enabled(&p, &event, true)?,
                HooksAction::Disable { event } => commands::hooks::cmd_set_enabled(&p, &event, false)?,
            }
        }
        Commands::Brief { path, action } => {
            let p = resolve(path);
            match action {
                BriefAction::Refresh => commands::brief::cmd_refresh(&p)?,
                BriefAction::Show => commands::brief::cmd_show(&p)?,
                BriefAction::Doctor => commands::brief::cmd_doctor(&p)?,
            }
        }
        Commands::Mcp { path } => {
            mcp::serve(&resolve(path))?;
        }
        Commands::Watch { path } => {
            commands::watch::watch(&resolve(path), &marker)?;
        }
    }

    Ok(())
}
