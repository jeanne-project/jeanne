use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Interface en ligne de commande pour Jeanne - Second Cerveau Hybride
#[derive(Parser, Debug)]
#[command(
    name = "jeanne",
    author = "Jeanne Project Contributors",
    version,
    about = "CLI headless pour Jeanne - Second Cerveau Hybride",
    long_about = "CLI headless pour l'indexation, la recherche FTS5 et l'audit d'intégrité de coffres Markdown Jeanne."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Commandes de gestion du coffre Markdown
    Vault(VaultCommand),
}

#[derive(Args, Debug)]
pub struct VaultCommand {
    #[command(subcommand)]
    pub action: VaultAction,
}

#[derive(Subcommand, Debug)]
pub enum VaultAction {
    /// Indexe récursivement les notes Markdown d'un coffre dans la base SQLite locale
    Index {
        /// Chemin racine du coffre (défaut : résolu via JEANNE_VAULT_PATH ou ~/Documents/JeanneVault)
        #[arg(long, short = 'p')]
        path: Option<PathBuf>,

        /// Sortie au format JSON structuré
        #[arg(long)]
        json: bool,
    },

    /// Recherche des notes dans le coffre par mots-clés ou question (FTS5)
    Search {
        /// Requête de recherche textuelle ou question en langage naturel
        query: String,

        /// Nombre maximal de résultats retournés (défaut : 10)
        #[arg(long, short = 'l', default_value_t = 10)]
        limit: usize,

        /// Chemin racine du coffre (défaut : résolu via JEANNE_VAULT_PATH ou ~/Documents/JeanneVault)
        #[arg(long, short = 'p')]
        path: Option<PathBuf>,

        /// Sortie au format JSON brut
        #[arg(long)]
        json: bool,
    },

    /// Contrôle l'intégrité du coffre et de la base SQLite
    Audit {
        /// Chemin racine du coffre (défaut : résolu via JEANNE_VAULT_PATH ou ~/Documents/JeanneVault)
        #[arg(long, short = 'p')]
        path: Option<PathBuf>,

        /// Sortie au format JSON structuré
        #[arg(long)]
        json: bool,
    },
}
