pub mod audit;
pub mod cli;
pub mod index;
pub mod search;
pub mod utils;

pub use audit::{
    AuditHealthStatus, AuditReport, BrokenLinkIssue, InvalidFrontmatterIssue, audit_vault,
};
pub use cli::{Cli, Commands, VaultAction, VaultCommand};
pub use index::{IndexSummary, index_vault};
pub use search::search_vault;
pub use utils::{format_timestamp, normalize_relative_path, resolve_effective_vault_path};
