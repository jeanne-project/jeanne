//! Tests TDD pour la découverte dynamique des modèles GGUF et la résolution multi-dossiers.
//! Correspond à la spécification `docs/specs/04c_SPEC_MODEL_DISCOVERY_AND_SETTINGS.md`.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;

use jeanne_core::local_llm::{LocalEngineConfig, LocalLlmEngine};
use jeanne_core::model_discovery::{
    discover_models_in_dirs, get_candidate_model_dirs, resolve_model_path_in_dirs,
};

/// Écrit un en-tête GGUF v2 valide minimal dans le fichier cible.
fn create_mock_gguf_file(path: &Path, dummy_body_bytes: usize) {
    let mut file = File::create(path).expect("Impossible de créer le fichier mock GGUF");
    // Magic "GGUF"
    file.write_all(b"GGUF").unwrap();
    // Version 2 (u32 little-endian)
    file.write_all(&2u32.to_le_bytes()).unwrap();
    // Tensor count 10 (u64 little-endian)
    file.write_all(&10u64.to_le_bytes()).unwrap();
    // Metadata KV count 0 (u64 little-endian)
    file.write_all(&0u64.to_le_bytes()).unwrap();
    // Rembourrage pour simuler la taille du fichier
    if dummy_body_bytes > 0 {
        let zeroes = vec![0u8; dummy_body_bytes];
        file.write_all(&zeroes).unwrap();
    }
}

#[test]
fn test_04c_01_candidate_model_dirs() {
    let dirs = get_candidate_model_dirs();
    assert!(
        !dirs.is_empty(),
        "La liste des répertoires candidats ne doit pas être vide"
    );

    // Doit contenir au moins un dossier finissant par 'models'
    let has_models_dir = dirs.iter().any(|d| {
        d.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
            == "models"
    });
    assert!(
        has_models_dir,
        "Au moins un dossier candidat doit être un sous-dossier 'models'"
    );
}

#[test]
fn test_04c_02_discover_models_finds_gguf() {
    let temp_dir = tempdir().expect("tempdir");
    let model_file = temp_dir.path().join("qwen2.5-3b-instruct-q4_k_m.gguf");
    create_mock_gguf_file(&model_file, 1024 * 1024); // 1 Mo

    let text_file = temp_dir.path().join("notes.txt");
    std::fs::write(&text_file, "Ce n'est pas un modèle GGUF").unwrap();

    let models = discover_models_in_dirs(&[temp_dir.path().to_path_buf()], None);

    assert_eq!(models.len(), 1, "Seul le fichier .gguf doit être détecté");
    let model = &models[0];
    assert_eq!(model.name, "qwen2.5-3b-instruct-q4_k_m.gguf");
    assert!(model.size_bytes > 1_000_000);
    assert!(!model.size_formatted.is_empty());
    assert!(!model.is_loaded);
    assert!(model.fits_ram);
}

#[test]
fn test_04c_03_discover_models_case_insensitivity() {
    let temp_dir = tempdir().expect("tempdir");
    let model_1 = temp_dir.path().join("qwen2.5-3b-instruct-q4_k_m.gguf");
    create_mock_gguf_file(&model_1, 2048);

    let model_2 = temp_dir.path().join("LLAMA-3.2-3B-INSTRUCT.GGUF");
    create_mock_gguf_file(&model_2, 4096);

    let models = discover_models_in_dirs(
        &[temp_dir.path().to_path_buf()],
        Some(&model_1.to_string_lossy()),
    );

    assert_eq!(
        models.len(),
        2,
        "Les extensions .gguf et .GGUF doivent être détectées"
    );

    // Le premier doit être marqué comme chargé
    let loaded_model = models
        .iter()
        .find(|m| m.name == "qwen2.5-3b-instruct-q4_k_m.gguf")
        .unwrap();
    assert!(
        loaded_model.is_loaded,
        "Le modèle correspondant au chemin actif doit avoir is_loaded = true"
    );

    let other_model = models
        .iter()
        .find(|m| m.name == "LLAMA-3.2-3B-INSTRUCT.GGUF")
        .unwrap();
    assert!(!other_model.is_loaded);
}

#[test]
fn test_04c_04_resolve_model_path_fallback_qwen() {
    let temp_dir = tempdir().expect("tempdir");
    let model_file = temp_dir.path().join("qwen2.5-3b-instruct-q4_k_m.gguf");
    create_mock_gguf_file(&model_file, 512);

    // Même avec requested = None, il doit trouver le fichier qwen2.5 dans le dossier candidat
    let resolved = resolve_model_path_in_dirs(None, &[temp_dir.path().to_path_buf()]);
    assert!(
        resolved.is_some(),
        "Doit résoudre automatiquement le fichier Qwen présent"
    );
    assert_eq!(resolved.unwrap(), model_file);
}

#[test]
fn test_04c_05_resolve_model_path_by_filename() {
    let temp_dir = tempdir().expect("tempdir");
    let model_file = temp_dir.path().join("Llama-3.2-3B-Instruct.gguf");
    create_mock_gguf_file(&model_file, 512);

    // Recherche insensible à la casse par nom de fichier
    let resolved = resolve_model_path_in_dirs(
        Some("llama-3.2-3b-instruct.gguf"),
        &[temp_dir.path().to_path_buf()],
    );
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap(), model_file);
}

#[tokio::test]
async fn test_04c_06_load_local_engine_with_discovered_model() {
    let temp_dir = tempdir().expect("tempdir");
    let model_file = temp_dir.path().join("qwen2.5-3b-instruct-q4_k_m.gguf");
    create_mock_gguf_file(&model_file, 1024);

    let config = LocalEngineConfig {
        model_path: Some(model_file.to_string_lossy().to_string()),
        context_size: 2048,
        threads: None,
        use_vulkan: false,
        expected_sha256: None,
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    assert!(!engine.is_model_loaded().await);

    let load_res = engine.load_model(None).await;
    assert!(
        load_res.is_ok(),
        "Le chargement du modèle mock doit réussir : {:?}",
        load_res.err()
    );
    assert!(engine.is_model_loaded().await);

    let stats = engine.get_stats().await;
    assert!(stats.memory_allocated_mb > 0);

    engine.unload_model().await.unwrap();
    assert!(!engine.is_model_loaded().await);
}
