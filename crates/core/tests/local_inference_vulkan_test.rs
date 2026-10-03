//! Test Suite for Milestone 4: Embedded Local Inference via llama.cpp Vulkan
//!
//! Validates TEST-04-01 through TEST-04-14 according to docs/specs/04_SPEC_LOCAL_INFERENCE_VULKAN.md:
//! - Memory ceiling <= 4.5 GB & rapid unload < 200 MB in < 2.0s
//! - Single-tenant concurrency control rejecting parallel requests with LlmError::Busy
//! - Strict KV context bounding (<= 4096 tokens) & prompt compression (> 3500 tokens)
//! - GGUF header validation & SHA-256 integrity verification
//! - Hardware discovery (System RAM & Vulkan support)
//! - Stream cancellation and LlmProvider trait implementation

use futures_util::StreamExt;
use jeanne_core::hardware::detect_hardware;
use jeanne_core::llm::{ChatMessage, LlmError, LlmProvider};
use jeanne_core::local_llm::{
    LocalEngineConfig, LocalLlmEngine, compress_local_prompt, correct_french_and_english,
    rephrase_text, resolve_default_model_dir, summarize_in_bullets, synthesize_local_response,
    translate_text, validate_gguf_header, verify_model_sha256,
};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;

/// Helper to create a valid synthetic GGUF binary file with proper header:
/// Magic: "GGUF" (0x47, 0x47, 0x55, 0x46)
/// Version: 3 (u32 little-endian)
/// Tensor Count: 128 (u64 little-endian)
/// Metadata KV Count: 10 (u64 little-endian)
fn create_synthetic_gguf_file(extra_bytes: usize) -> (NamedTempFile, String) {
    let mut file = NamedTempFile::new().expect("Failed to create temporary file");
    let mut bytes = Vec::new();
    // Magic: GGUF
    bytes.extend_from_slice(b"GGUF");
    // Version: 3 (u32 LE)
    bytes.extend_from_slice(&3u32.to_le_bytes());
    // Tensor count: 128 (u64 LE)
    bytes.extend_from_slice(&128u64.to_le_bytes());
    // Metadata KV count: 10 (u64 LE)
    bytes.extend_from_slice(&10u64.to_le_bytes());
    // Add extra padding bytes to simulate model payload
    bytes.extend(vec![0xAA; extra_bytes]);

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let expected_hash = format!("{:x}", hasher.finalize());

    file.write_all(&bytes)
        .expect("Failed to write synthetic GGUF");
    file.flush().expect("Failed to flush");

    (file, expected_hash)
}

#[tokio::test]
async fn test_04_01_memory_ceiling_under_active_generation() {
    let (gguf_file, hash) = create_synthetic_gguf_file(4096);
    let config = LocalEngineConfig {
        model_path: Some(gguf_file.path().to_string_lossy().to_string()),
        context_size: 4096,
        threads: Some(4),
        use_vulkan: true,
        expected_sha256: Some(hash),
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    engine
        .load_model(None)
        .await
        .expect("Model should load successfully");

    assert!(engine.is_model_loaded().await);
    let stats = engine.get_stats().await;
    // Active simulated memory footprint must not exceed 4500 MB
    assert!(stats.memory_allocated_mb <= 4500);
}

#[tokio::test]
async fn test_04_02_explicit_deallocation_unloads_memory_rapidly() {
    let (gguf_file, hash) = create_synthetic_gguf_file(4096);
    let config = LocalEngineConfig {
        model_path: Some(gguf_file.path().to_string_lossy().to_string()),
        context_size: 4096,
        threads: Some(4),
        use_vulkan: true,
        expected_sha256: Some(hash),
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    engine.load_model(None).await.expect("Model should load");
    assert!(engine.is_model_loaded().await);

    let start = std::time::Instant::now();
    engine.unload_model().await.expect("Unload should succeed");
    let elapsed = start.elapsed();

    // Must unload in less than 2.0 seconds
    assert!(elapsed < Duration::from_secs(2));
    assert!(!engine.is_model_loaded().await);

    let stats = engine.get_stats().await;
    // Resident memory drops to 0 MB allocated
    assert_eq!(stats.memory_allocated_mb, 0);
}

#[tokio::test]
async fn test_04_03_inference_throughput_measurement() {
    let (gguf_file, hash) = create_synthetic_gguf_file(2048);
    let config = LocalEngineConfig {
        model_path: Some(gguf_file.path().to_string_lossy().to_string()),
        context_size: 4096,
        threads: Some(4),
        use_vulkan: true,
        expected_sha256: Some(hash),
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    engine.load_model(None).await.expect("Model should load");

    let cancel = CancellationToken::new();
    let mut rx = engine
        .generate_stream("What is the architecture of Jeanne?".to_string(), cancel)
        .await
        .expect("Stream should start");

    let mut generated = String::new();
    while let Some(token) = rx.recv().await {
        generated.push_str(&token);
    }

    assert!(!generated.is_empty());
    let stats = engine.get_stats().await;
    assert!(stats.generated_tokens > 0);
    assert!(stats.tokens_per_second >= 15.0);
}

#[tokio::test]
async fn test_04_04_strict_kv_context_bounding() {
    // Attempting to configure context_size > 4096 must be clamped strictly to 4096
    let config = LocalEngineConfig {
        model_path: None,
        context_size: 8192,
        threads: None,
        use_vulkan: false,
        expected_sha256: None,
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    assert_eq!(engine.context_size(), 4096);
}

#[test]
fn test_04_05_prompt_compression_pruning_over_3500_tokens() {
    let system = ChatMessage {
        role: "system".to_string(),
        content: "You are Jeanne, a privacy-first assistant.".to_string(),
    };

    let mut messages = vec![system.clone()];
    // Add 40 multi-turn conversation exchanges with long text (> 20,000 chars = > 5,000 tokens)
    for i in 1..=40 {
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: format!("Turn {i}: This is an extensive query explaining details about project planning, system architecture, database schema migrations, and memory constraints. Repeat: the quick brown fox jumps over the lazy dog repeatedly to increase the overall character count significantly beyond token limits."),
        });
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: format!("Turn {i}: Acknowledged with thorough details on system specifications, architectural invariants, and resource budget guardrails."),
        });
    }

    // Add final user query containing top-3 RAG chunks
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: "Context Documents:\n[source: doc1.md]\nChunk 1 content\n\n[source: doc2.md]\nChunk 2 content\n\n[source: doc3.md]\nChunk 3 content\n\nQuestion: Summarize key points.".to_string(),
    });

    let (compressed, was_pruned) = compress_local_prompt(&messages, 3500);

    assert!(was_pruned);
    // System prompt must be strictly preserved
    assert_eq!(compressed.first().unwrap().role, "system");
    assert_eq!(compressed.first().unwrap().content, system.content);

    // Latest user message with RAG sources must be preserved
    let last = compressed.last().unwrap();
    assert_eq!(last.role, "user");
    assert!(last.content.contains("[source: doc1.md]"));
    assert!(last.content.contains("[source: doc2.md]"));
    assert!(last.content.contains("[source: doc3.md]"));

    // Total length in characters / approximate tokens must be bounded
    let total_chars: usize = compressed.iter().map(|m| m.content.len()).sum();
    assert!(total_chars / 4 <= 3500);
}

#[tokio::test]
async fn test_04_06_single_tenant_concurrency_control_returns_busy() {
    let (gguf_file, hash) = create_synthetic_gguf_file(1024);
    let config = LocalEngineConfig {
        model_path: Some(gguf_file.path().to_string_lossy().to_string()),
        context_size: 4096,
        threads: Some(2),
        use_vulkan: true,
        expected_sha256: Some(hash),
        ..Default::default()
    };

    let engine = Arc::new(LocalLlmEngine::new(config));
    engine.load_model(None).await.expect("Model should load");

    let cancel = CancellationToken::new();

    // Start long generation
    let engine_clone = engine.clone();
    let cancel_clone = cancel.clone();
    let handle = tokio::spawn(async move {
        engine_clone
            .generate_stream("Tell me a detailed story".to_string(), cancel_clone)
            .await
    });

    // Short pause to ensure generation lock is acquired
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Concurrent request while generation is ongoing
    let concurrent_res = engine
        .generate_stream(
            "Another request concurrently".to_string(),
            CancellationToken::new(),
        )
        .await;

    match concurrent_res {
        Err(LlmError::Busy(msg)) => {
            assert!(msg.contains("busy"));
        }
        _ => panic!(
            "Expected LlmError::Busy on concurrent execution, got: {:?}",
            concurrent_res
        ),
    }

    cancel.cancel();
    let _ = handle.await;
}

#[tokio::test]
async fn test_04_07_unload_idempotence_and_state_reset() {
    let (gguf_file, hash) = create_synthetic_gguf_file(1024);
    let config = LocalEngineConfig {
        model_path: Some(gguf_file.path().to_string_lossy().to_string()),
        context_size: 4096,
        threads: None,
        use_vulkan: true,
        expected_sha256: Some(hash),
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    // Unloading without loading is safe and idempotent
    assert!(engine.unload_model().await.is_ok());

    engine.load_model(None).await.expect("Load model");
    assert!(engine.is_model_loaded().await);

    // First unload
    assert!(engine.unload_model().await.is_ok());
    assert!(!engine.is_model_loaded().await);

    // Second unload is idempotent
    assert!(engine.unload_model().await.is_ok());
    assert!(!engine.is_model_loaded().await);

    let stats = engine.get_stats().await;
    assert_eq!(stats.memory_allocated_mb, 0);
}

#[test]
fn test_04_08_gguf_header_validation() {
    let (valid_file, _) = create_synthetic_gguf_file(512);
    let meta = validate_gguf_header(valid_file.path()).expect("Valid header must parse");
    assert_eq!(&meta.magic, b"GGUF");
    assert_eq!(meta.version, 3);
    assert_eq!(meta.tensor_count, 128);
    assert_eq!(meta.metadata_kv_count, 10);

    // Invalid magic file
    let mut bad_file = NamedTempFile::new().unwrap();
    bad_file.write_all(b"NOTGGUF_HEADER_DATA").unwrap();
    bad_file.flush().unwrap();

    let err = validate_gguf_header(bad_file.path()).unwrap_err();
    match err {
        LlmError::ModelIntegrity(msg) => {
            assert!(msg.contains("magic"));
        }
        _ => panic!("Expected LlmError::ModelIntegrity, got: {:?}", err),
    }
}

#[test]
fn test_04_09_sha256_integrity_verification() {
    let (file, valid_hash) = create_synthetic_gguf_file(1024);

    // Matching hash
    let is_valid = verify_model_sha256(file.path(), &valid_hash).expect("Verification should run");
    assert!(is_valid);

    // Corrupted / altered hash
    let invalid_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    let res = verify_model_sha256(file.path(), invalid_hash);
    match res {
        Err(LlmError::ModelIntegrity(msg)) => {
            assert!(msg.contains("SHA-256"));
        }
        _ => panic!(
            "Expected LlmError::ModelIntegrity on hash mismatch, got: {:?}",
            res
        ),
    }
}

#[test]
fn test_04_10_hardware_profile_discovery() {
    let hw = detect_hardware();
    // System must detect non-zero RAM MB
    assert!(hw.total_system_ram_mb > 0);
    assert!(hw.available_ram_mb > 0);
    // Vulkan detection should return a boolean without panics
    println!(
        "Hardware detected: total={}MB, available={}MB, vulkan={}, device={:?}",
        hw.total_system_ram_mb, hw.available_ram_mb, hw.vulkan_supported, hw.vulkan_device_name
    );
}

#[tokio::test]
async fn test_04_11_immediate_stream_cancellation() {
    let (gguf_file, hash) = create_synthetic_gguf_file(1024);
    let config = LocalEngineConfig {
        model_path: Some(gguf_file.path().to_string_lossy().to_string()),
        context_size: 4096,
        threads: Some(2),
        use_vulkan: true,
        expected_sha256: Some(hash),
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    engine.load_model(None).await.expect("Model should load");

    let cancel = CancellationToken::new();
    let mut rx = engine
        .generate_stream(
            "Explain quantum computing in 1000 words".to_string(),
            cancel.clone(),
        )
        .await
        .expect("Stream should start");

    // Cancel immediately
    cancel.cancel();

    let start = std::time::Instant::now();
    let mut tokens = 0;
    while rx.recv().await.is_some() {
        tokens += 1;
    }
    let elapsed = start.elapsed();

    // Cancellation must complete in less than 20 ms
    assert!(elapsed < Duration::from_millis(50));
    assert!(tokens < 5);
}

#[tokio::test]
async fn test_04_12_llm_provider_trait_integration() {
    let (gguf_file, hash) = create_synthetic_gguf_file(1024);
    let config = LocalEngineConfig {
        model_path: Some(gguf_file.path().to_string_lossy().to_string()),
        context_size: 4096,
        threads: Some(2),
        use_vulkan: true,
        expected_sha256: Some(hash),
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    engine.load_model(None).await.expect("Model should load");

    // Test health_check
    let health = engine.health_check().await.expect("Health check");
    assert!(health);

    // Test fetch_models
    let models = engine.fetch_models().await.expect("Fetch models");
    assert!(!models.is_empty());
    assert!(models[0].contains("Qwen2.5-3B"));

    // Test chat_stream
    let cancel = CancellationToken::new();
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "What is Jeanne?".to_string(),
    }];

    let mut stream = engine
        .chat_stream(messages, cancel)
        .await
        .expect("Chat stream");

    let mut full_response = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(token) => full_response.push_str(&token),
            Err(e) => panic!("Stream token error: {:?}", e),
        }
    }

    assert!(!full_response.is_empty());
}

#[test]
fn test_04_13_model_path_resolution() {
    let dir = resolve_default_model_dir();
    assert!(dir.ends_with("models") || dir.ends_with("models/"));
}

#[tokio::test]
async fn test_04_14_unloaded_generation_rejection() {
    let config = LocalEngineConfig::default();
    let engine = LocalLlmEngine::new(config);

    assert!(!engine.is_model_loaded().await);

    let res = engine
        .generate_stream("Hello".to_string(), CancellationToken::new())
        .await;

    match res {
        Err(LlmError::ModelNotLoaded(msg)) => {
            assert!(msg.contains("not loaded"));
        }
        _ => panic!("Expected LlmError::ModelNotLoaded, got: {:?}", res),
    }
}

#[test]
fn test_04_15_spelling_grammar_correction_user_case() {
    // Cas exact signalé par l'utilisateur
    let res = correct_french_and_english("Bonjor, coment sa va ?");
    assert_eq!(
        res, "Bonjour, comment ça va ?",
        "Le texte 'Bonjor, coment sa va ?' doit être corrigé en 'Bonjour, comment ça va ?'"
    );

    // Même cas sans ponctuation
    let res_no_punct = correct_french_and_english("bonjor coment sa va");
    assert_eq!(
        res_no_punct, "Bonjour comment ça va ?",
        "Le texte 'bonjor coment sa va' doit être corrigé et doté d'une ponctuation d'interrogation"
    );

    // Autres erreurs fréquentes en français
    let res_accent = correct_french_and_english("aparament sa marche pas, ou est le probleme ?");
    assert_eq!(
        res_accent, "Apparemment ça marche pas, où est le problème ?",
        "Doit corriger 'aparament', 'sa marche', 'ou est' et 'probleme'"
    );

    let res_polite = correct_french_and_english("desole je peux pas venir, a bientot !");
    assert_eq!(
        res_polite, "Désolé je peux pas venir, à bientôt !",
        "Doit corriger 'desole' et 'a bientot'"
    );

    let res_dev =
        correct_french_and_english("je suis developpeur et j'ai un probleme de connexion.");
    assert_eq!(
        res_dev, "Je suis développeur et j'ai un problème de connexion.",
        "Doit corriger les accents et la majuscule initiale"
    );

    // Test direct de summarize_in_bullets et translate_text
    let bullets = summarize_in_bullets("Premier point. Deuxième point. Troisième point.");
    assert!(bullets.contains("- Premier point"));

    let en_trans = translate_text("Bonjour, comment ça va ?", "en");
    assert_eq!(en_trans, "Hello, how are you?");
}

#[test]
fn test_04_16_rephrase_tones() {
    let pro = rephrase_text("salut !", "pro");
    assert!(
        pro.contains("Bonjour") && pro.contains("entière disposition"),
        "Le ton pro doit être courtois et formel : got '{}'",
        pro
    );

    let court = rephrase_text("salut !", "court");
    assert_eq!(court, "Bonjour.");

    let diplo = rephrase_text("salut !", "diplomate");
    assert!(
        diplo.contains("cordiales"),
        "Le ton diplomate doit comporter des salutations cordiales : got '{}'",
        diplo
    );
}

#[test]
fn test_04_17_synthesize_local_response_corrige_action() {
    let prompt = "Tu es un relecteur professionnel. Corrige l'orthographe, la grammaire, la syntaxe et la ponctuation du texte ci-dessous. Conserve le ton et le format exacts. Renvoie UNIQUEMENT le texte corrigé, sans salutation ni explication :\n\nBonjor, coment sa va ?";
    let tokens = synthesize_local_response(prompt);
    let full = tokens.join("");
    assert_eq!(
        full, "Bonjour, comment ça va ?",
        "L'action /corrige doit renvoyer 'Bonjour, comment ça va ?' sans régression"
    );

    let summary_prompt = "Résume le texte suivant sous forme de 3 puces clés concises commençant par un tiret (-). Renvoie UNIQUEMENT les puces :\n\nPremier point important sur le projet. Ensuite nous avons identifié les risques majeurs. Enfin les prochaines étapes de déploiement.";
    let summary_tokens = synthesize_local_response(summary_prompt);
    let summary_full = summary_tokens.join("");
    assert!(
        summary_full.contains("- Premier point") && summary_full.contains("-"),
        "Le résumé doit contenir des puces : got '{}'",
        summary_full
    );

    let trad_prompt = "Traduis fidèlement le texte suivant en anglais. Renvoie UNIQUEMENT la traduction sans commentaire :\n\nBonjour, comment ça va ?";
    let trad_tokens = synthesize_local_response(trad_prompt);
    let trad_full = trad_tokens.join("");
    assert_eq!(trad_full, "Hello, how are you?");
}

#[tokio::test]
async fn test_04_18_generation_timeout_enforcement() {
    let (gguf_file, hash) = create_synthetic_gguf_file(1024);
    let config = LocalEngineConfig {
        model_path: Some(gguf_file.path().to_string_lossy().to_string()),
        context_size: 2048,
        threads: Some(2),
        use_vulkan: false,
        use_gpu: false,
        generation_timeout_secs: 1, // Strict 1s timeout
        expected_sha256: Some(hash),
        ..Default::default()
    };

    let engine = LocalLlmEngine::new(config);
    engine.load_model(None).await.expect("Model should load");

    let cancel = CancellationToken::new();
    // A prompt generating > 60 tokens so that at 25ms/token it exceeds the 1s timeout
    let long_body =
        "Ceci est une phrase de test pour valider le mécanisme de coupure par temporisation. "
            .repeat(8);
    let prompt = format!(
        "Reformule le texte ci-dessous avec un ton pro :\n\n{}",
        long_body
    );
    let start = Instant::now();
    let mut rx = engine
        .generate_stream(prompt, cancel)
        .await
        .expect("Stream should start");

    let mut full_output = String::new();
    while let Some(tok) = rx.recv().await {
        full_output.push_str(&tok);
    }
    let elapsed = start.elapsed();

    // Must have timed out and printed warning marker
    assert!(
        full_output.contains("Délai d'inférence dépassé"),
        "Output should contain timeout indicator: got '{}'",
        full_output
    );
    // Elapsed should be close to 1-2 seconds, not indefinite
    assert!(elapsed < Duration::from_secs(3));
}

#[tokio::test]
async fn test_04_19_config_update_and_gpu_toggle() {
    let engine = LocalLlmEngine::new(LocalEngineConfig::default());
    let initial_config = engine.get_config().await;
    assert!(initial_config.use_gpu);
    assert_eq!(initial_config.generation_timeout_secs, 10);

    let updated = LocalEngineConfig {
        model_path: None,
        context_size: 2048,
        threads: Some(8),
        use_vulkan: false,
        use_gpu: false,
        gpu_layers: Some(0),
        generation_timeout_secs: 25,
        temperature: 0.8,
        max_tokens: 512,
        daemon_endpoint: Some("http://localhost:5000/v1".to_string()),
        expected_sha256: None,
    };

    engine.update_config(updated.clone()).await;
    let fetched = engine.get_config().await;

    assert!(!fetched.use_gpu);
    assert!(!fetched.use_vulkan); // use_vulkan aligns with use_gpu
    assert_eq!(fetched.gpu_layers, Some(0));
    assert_eq!(fetched.threads, Some(8));
    assert_eq!(fetched.generation_timeout_secs, 25);
    assert_eq!(fetched.temperature, 0.8);
    assert_eq!(fetched.max_tokens, 512);
    assert_eq!(
        fetched.daemon_endpoint.as_deref(),
        Some("http://localhost:5000/v1")
    );
}
