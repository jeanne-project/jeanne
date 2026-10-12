use clap::Parser;
use jeanne_cli::audit::audit_vault;
use jeanne_cli::cli::{Cli, Commands, VaultAction};
use jeanne_cli::index::index_vault;
use jeanne_cli::search::search_vault;
use jeanne_cli::utils::resolve_effective_vault_path;
use std::process::ExitCode;

fn run() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Vault(vault_cmd) => match vault_cmd.action {
            VaultAction::Index { path, json } => {
                let vault_path = resolve_effective_vault_path(path.as_deref())?;
                index_vault(&vault_path, json)?;
                Ok(ExitCode::SUCCESS)
            }
            VaultAction::Search {
                query,
                limit,
                path,
                json,
            } => {
                let vault_path = resolve_effective_vault_path(path.as_deref())?;
                search_vault(&vault_path, &query, limit, json)?;
                Ok(ExitCode::SUCCESS)
            }
            VaultAction::Audit { path, json } => {
                let vault_path = resolve_effective_vault_path(path.as_deref())?;
                audit_vault(&vault_path, json)?;
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("Erreur : {err:#}");
            ExitCode::FAILURE
        }
    }
}
