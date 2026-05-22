mod cli;
mod commands;
mod core;
mod mcp;
mod mesh;

use clap::Parser;
use cli::{Cli, Commands, DecisionAction, MessageAction, ProposalAction, StateAction, TaskAction};
use cli::{AgentsAction, BriefAction, ConfigAction, HooksAction, RoleAction, StyleAction};

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
        Commands::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
        }
        Commands::Status { path } => {
            core::vault::status(&resolve(path))?;
        }
        Commands::Doctor { path, pending_proposals } => {
            commands::doctor::doctor(&resolve(path), &marker, pending_proposals)?;
        }
        Commands::Handoff { to, path, print, save, fill_summary } => {
            commands::handoff::handoff(
                &to,
                &resolve(path),
                print,
                save.as_deref(),
                fill_summary,
            )?;
        }
        Commands::Messages { path, action } => {
            let p = resolve(path);
            match action {
                MessageAction::List {
                    from,
                    to,
                    status,
                    unread_for,
                    limit,
                } => commands::messages::cmd_list(
                    &p,
                    commands::messages::MessageFilter {
                        from,
                        to,
                        status,
                        unread_for,
                        limit,
                    },
                )?,
                MessageAction::Ack { ids, by } => commands::messages::cmd_ack(&p, &ids, &by)?,
            }
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
                DecisionAction::SetStatus { ids, status } => {
                    commands::decision::cmd_set_status(&p, &ids, &status)?;
                }
            }
        }
        Commands::Proposal { path, action } => {
            let p = resolve(path);
            match action {
                ProposalAction::List => commands::proposal::cmd_list(&p)?,
                ProposalAction::Show { ids } => commands::proposal::cmd_show(&p, &ids)?,
                ProposalAction::Apply { ids } => commands::proposal::cmd_apply(&p, &ids)?,
                ProposalAction::Reject { ids } => commands::proposal::cmd_reject(&p, &ids)?,
                ProposalAction::Gc { statuses, keep, project } => {
                    commands::proposal::cmd_gc(&p, statuses, keep, project)?;
                }
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
        Commands::Serve { port } => {
            let root = resolve(None);
            crate::mesh::init_pool(&cfg.mesh);
            if let Some(lp) = cfg.mesh.listen_port {
                if lp != port {
                    eprintln!("[mesh] note: listen_port {} != serve port {}", lp, port);
                }
            }
            let vault_root = std::path::PathBuf::from(&root);
            tokio::runtime::Runtime::new()?.block_on(async {
                crate::mesh::serve::serve(crate::mesh::serve::ServeConfig {
                    vault_root,
                    port,
                }).await
            })?;
        }
        Commands::Mcp { path } => {
            mcp::serve(&resolve(path))?;
        }
        Commands::Watch { path } => {
            commands::watch::watch(&resolve(path), &marker)?;
        }
        Commands::Task { path, action } => {
            let p = resolve(path);
            match action {
                TaskAction::List => commands::plan::cmd_list(&p)?,
                TaskAction::Claim { id, agent } => commands::plan::cmd_claim(&p, &id, &agent)?,
                TaskAction::Release { id, agent } => {
                    commands::plan::cmd_release(&p, &id, &agent)?;
                }
            }
        }
        Commands::Unlock { path, all, file } => {
            let p = resolve(path);
            if all {
                commands::unlock::unlock_all(&p)?;
            } else if let Some(f) = file {
                commands::unlock::unlock_file(&p, &f)?;
            } else {
                anyhow::bail!("specify --all or --file <path>");
            }
        }
        Commands::Gc { path, dry_run } => {
            commands::gc::run(&resolve(path), dry_run)?;
        }
        Commands::Style { path, action } => {
            let p = resolve(path);
            match action {
                StyleAction::List => commands::style::cmd_list(&p)?,
                StyleAction::Show { lang } => commands::style::cmd_show(&p, &lang)?,
                StyleAction::Doctor { check_fmt } => commands::style::cmd_doctor(&p, check_fmt)?,
            }
        }
        Commands::Config { action } => match action {
            ConfigAction::Show => commands::config::cmd_show()?,
            ConfigAction::Set { key, value } => commands::config::cmd_set(&key, &value)?,
            ConfigAction::Get { key } => commands::config::cmd_get(&key)?,
        },
    }

    Ok(())
}
