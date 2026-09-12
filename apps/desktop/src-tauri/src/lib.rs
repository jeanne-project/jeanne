#[allow(unused_imports)]
use tauri::Manager;

#[tauri::command]
fn get_core_version() -> String {
    jeanne_core::version().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();
    tracing::info!("Démarrage du client Jeanne Desktop...");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![get_core_version])
        .setup(|_app| {
            tracing::info!("Sous-système bureau Tauri initialisé.");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Erreur lors de l'exécution de Jeanne Desktop");
}
