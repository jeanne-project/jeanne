use anyhow::Result;
use jeanne_cli::audit::{AuditHealthStatus, audit_vault};
use jeanne_cli::index::index_vault;
use jeanne_cli::search::search_vault;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_vault_index_and_search() -> Result<()> {
    let dir = tempdir()?;
    let vault_path = dir.path();

    // 1. Préparation de fichiers Markdown dans le coffre
    let note1_path = vault_path.join("note1.md");
    let note1_content = r#"---
id: note-1
title: Architecture Hybride Jeanne
date_creation: "2026-09-12T00:00:00Z"
type: concept
status: actif
---
Le système Jeanne utilise un stockage hybride performant combinant SQLite FTS5 et sqlite-vec.
"#;
    fs::write(&note1_path, note1_content)?;

    let subfolder = vault_path.join("protocols");
    fs::create_dir_all(&subfolder)?;
    let note2_path = subfolder.join("ipc.md");
    let note2_content = r#"---
id: note-2
title: Communication IPC
date_creation: "2026-09-13T10:00:00Z"
type: procedural
status: actif
---
La communication inter-processus des plugins polyglottes repose sur JSON-RPC 2.0 via standard I/O.
"#;
    fs::write(&note2_path, note2_content)?;

    // Fichiers devant être ignorés (dossier caché et dossier .jeanne)
    let dot_folder = vault_path.join(".hidden");
    fs::create_dir_all(&dot_folder)?;
    fs::write(dot_folder.join("ignored.md"), "Fichier caché ignoré")?;

    let jeanne_folder = vault_path.join(".jeanne");
    fs::create_dir_all(&jeanne_folder)?;
    fs::write(jeanne_folder.join("internal.md"), "Fichier interne ignoré")?;

    // 2. Indexation du coffre
    let summary = index_vault(vault_path, false)?;
    assert_eq!(summary.indexed_files, 2);
    assert_eq!(summary.indexed_chunks, 2);
    assert_eq!(summary.links_created, 0);

    // 3. Recherche FTS5 par mots-clés
    let results = search_vault(vault_path, "hybride", 5, false)?;
    assert!(
        !results.is_empty(),
        "La recherche FTS5 devrait trouver au moins un résultat"
    );
    assert_eq!(results[0].file_path, "note1.md");
    assert!(results[0].snippet.contains("<mark>hybride</mark>"));

    // 4. Recherche FTS5 dans un sous-dossier
    let results_ipc = search_vault(vault_path, "JSON-RPC", 5, true)?;
    assert!(!results_ipc.is_empty());
    assert_eq!(results_ipc[0].file_path, "protocols/ipc.md");

    // 5. Repli recherche par question naturelle
    let results_q = search_vault(
        vault_path,
        "Comment fonctionne le stockage hybride ?",
        5,
        false,
    )?;
    assert!(!results_q.is_empty());
    assert_eq!(results_q[0].file_path, "note1.md");

    Ok(())
}

#[test]
fn test_vault_audit_health_and_sync() -> Result<()> {
    let dir = tempdir()?;
    let vault_path = dir.path();

    let note_path = vault_path.join("concept.md");
    let note_content = r#"---
id: concept-1
title: Concepts Clés
date_creation: "2026-09-12T00:00:00Z"
status: actif
---
Contenu initial indexé.
"#;
    fs::write(&note_path, note_content)?;

    // Indexation initiale
    let summary = index_vault(vault_path, false)?;
    assert_eq!(summary.indexed_files, 1);

    // Audit initial : coffre propre
    let report = audit_vault(vault_path, false)?;
    assert_eq!(report.status, AuditHealthStatus::Ok);
    assert_eq!(report.sqlite_integrity, "ok");
    assert_eq!(report.total_disk_files, 1);
    assert!(report.unindexed_files.is_empty());
    assert!(report.orphan_files.is_empty());
    assert!(report.outdated_files.is_empty());

    // 1. Ajout d'un nouveau fichier non indexé sur le disque
    let unindexed_path = vault_path.join("draft.md");
    fs::write(&unindexed_path, "# Nouveau brouillon non indexé")?;

    let report_unindexed = audit_vault(vault_path, false)?;
    assert_eq!(report_unindexed.status, AuditHealthStatus::Warning);
    assert_eq!(
        report_unindexed.unindexed_files,
        vec!["draft.md".to_string()]
    );

    // 2. Modification d'un fichier existant sans réindexer
    fs::write(
        &note_path,
        "Contenu modifié sur le disque sans passer par l'indexeur.",
    )?;
    let report_outdated = audit_vault(vault_path, false)?;
    assert!(
        report_outdated
            .outdated_files
            .contains(&"concept.md".to_string())
    );

    // 3. Suppression d'un fichier indexé (détection d'orphelin)
    fs::remove_file(&note_path)?;
    let report_orphan = audit_vault(vault_path, false)?;
    assert!(
        report_orphan
            .orphan_files
            .contains(&"concept.md".to_string())
    );

    Ok(())
}

#[test]
fn test_vault_audit_frontmatter_and_broken_links() -> Result<()> {
    let dir = tempdir()?;
    let vault_path = dir.path();

    // 1. Note avec lien superseded_by valide
    let target_path = vault_path.join("specs_v2.md");
    fs::write(
        &target_path,
        r#"---
title: Specs V2
---
Version 2 officielle.
"#,
    )?;

    let valid_link_path = vault_path.join("specs_v1.md");
    fs::write(
        &valid_link_path,
        r#"---
title: Specs V1
superseded_by: specs_v2.md
---
Ancienne version.
"#,
    )?;

    // 2. Note avec lien superseded_by brisé (cible inexistante)
    let broken_link_path = vault_path.join("legacy.md");
    fs::write(
        &broken_link_path,
        r#"---
title: Legacy
superseded_by: non_existent_future_spec.md
---
Note obsolète.
"#,
    )?;

    // 3. Note avec frontmatter corrompu
    let corrupted_path = vault_path.join("corrupted.md");
    fs::write(
        &corrupted_path,
        r#"---
title: [unclosed yaml list
invalid_yaml
---
Corps du document.
"#,
    )?;

    // Indexer le coffre
    index_vault(vault_path, false)?;

    // Audit du coffre
    let report = audit_vault(vault_path, false)?;
    assert_eq!(report.status, AuditHealthStatus::Error);
    assert_eq!(report.broken_links.len(), 1);
    assert_eq!(report.broken_links[0].source_path, "legacy.md");
    assert_eq!(
        report.broken_links[0].target_path,
        "non_existent_future_spec.md"
    );

    assert_eq!(report.invalid_frontmatters.len(), 1);
    assert_eq!(report.invalid_frontmatters[0].file_path, "corrupted.md");

    Ok(())
}

#[test]
fn test_cli_vault_json_output() -> Result<()> {
    let dir = tempdir()?;
    let vault_path = dir.path();

    let note_path = vault_path.join("welcome.md");
    fs::write(
        &note_path,
        r#"---
title: Welcome Note
date_creation: "2026-09-12T00:00:00Z"
---
Bienvenue dans le coffre Jeanne.
"#,
    )?;

    // Test sortie index JSON
    let summary = index_vault(vault_path, true)?;
    let summary_json = serde_json::to_string(&summary)?;
    assert!(summary_json.contains("\"indexed_files\":1"));

    // Test sortie search JSON
    let search_results = search_vault(vault_path, "Bienvenue", 10, true)?;
    assert_eq!(search_results.len(), 1);
    let search_json = serde_json::to_string(&search_results)?;
    assert!(search_json.contains("\"file_path\":\"welcome.md\""));

    // Test sortie audit JSON
    let audit_report = audit_vault(vault_path, true)?;
    let audit_json = serde_json::to_string(&audit_report)?;
    assert!(audit_json.contains("\"status\":\"ok\""));

    Ok(())
}

#[test]
fn test_cli_binary_process() -> Result<()> {
    let dir = tempdir()?;
    let vault_path = dir.path();

    let note_path = vault_path.join("intro.md");
    fs::write(
        &note_path,
        r#"---
title: Introduction Jeanne
---
Bienvenue dans le second cerveau local et souverain.
"#,
    )?;

    let bin_path = env!("CARGO_BIN_EXE_jeanne");

    let path_str = vault_path.to_string_lossy();

    // 1. jeanne vault index --path <dir> --json
    let index_output = std::process::Command::new(bin_path)
        .args(["vault", "index", "--path", path_str.as_ref(), "--json"])
        .output()?;
    assert!(index_output.status.success());
    let stdout_index = String::from_utf8_lossy(&index_output.stdout);
    assert!(stdout_index.contains("\"indexed_files\": 1"));

    // 2. jeanne vault search "souverain" --path <dir> --json
    let search_output = std::process::Command::new(bin_path)
        .args([
            "vault",
            "search",
            "souverain",
            "--path",
            path_str.as_ref(),
            "--json",
        ])
        .output()?;
    assert!(search_output.status.success());
    let stdout_search = String::from_utf8_lossy(&search_output.stdout);
    assert!(stdout_search.contains("intro.md"));

    // 3. jeanne vault audit --path <dir> --json
    let audit_output = std::process::Command::new(bin_path)
        .args(["vault", "audit", "--path", path_str.as_ref(), "--json"])
        .output()?;
    assert!(audit_output.status.success());
    let stdout_audit = String::from_utf8_lossy(&audit_output.stdout);
    assert!(stdout_audit.contains("\"status\": \"ok\""));

    Ok(())
}
