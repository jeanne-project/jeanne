fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "search_notes",
            "capture_quick_note",
            "open_note_in_editor",
            "get_vault_stats",
            "get_core_version",
            "set_quick_access_height",
            "exit_app",
        ]),
    ))
    .expect("failed to run tauri_build");
}
