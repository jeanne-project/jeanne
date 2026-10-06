//! Module de gestion et d'exécution des plugins sous-processus Jeanne.

pub mod manager;
pub mod models;
pub mod runner;

pub use manager::PluginManager;
pub use models::{
    JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, PluginCapability,
    PluginEntrypoint, PluginError, PluginLifecycle, PluginManifest,
};
pub use runner::execute_json_rpc;
