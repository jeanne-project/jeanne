use jeanne_core::plugins::{
    JsonRpcRequest, PluginCapability, PluginLifecycle, PluginManager, PluginManifest,
    execute_json_rpc,
};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn ensure_plugin_executable(manifest: &PluginManifest) -> Option<PathBuf> {
    if let Ok(exe) = PluginManager::resolve_executable(manifest) {
        return Some(exe);
    }

    // Auto-compilation transparente si main.go est présent et `go` est disponible
    let main_go = manifest.root_dir.join("main.go");
    if main_go.exists() {
        let rel_entrypoint = manifest.entrypoint.resolve_for_current_os();
        let target = manifest.root_dir.join(rel_entrypoint);
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let build_res = Command::new("go")
            .args(["build", "-ldflags=-s -w", "-o"])
            .arg(&target)
            .arg(".")
            .current_dir(&manifest.root_dir)
            .output();

        if let Ok(out) = build_res {
            if out.status.success() && target.exists() {
                return Some(target);
            }
        }
    }

    None
}

#[test]
fn test_plugin_discovery_and_manifest_parsing() {
    let mut manager = PluginManager::new();
    manager.discover_plugins();

    let plugins = manager.list_plugins();
    assert!(
        plugins.len() >= 5,
        "Au moins 5 plugins doivent être découverts dans le workspace (trouvés: {})",
        plugins.len()
    );

    // 1. Vérification du plugin llm-runner
    let llm_plugin = manager.get_plugin("org.jeanneproject.llm.runner");
    assert!(
        llm_plugin.is_some(),
        "Plugin llm-runner doit être découvert"
    );
    let llm = llm_plugin.unwrap();
    assert_eq!(llm.lifecycle, PluginLifecycle::DaemonManaged);
    assert!(
        llm.capabilities
            .iter()
            .any(|c| matches!(c, PluginCapability::LlmRunner { .. })),
        "llm-runner doit déclarer capability LlmRunner"
    );

    // 2. Vérification du plugin embeddings
    let emb_plugin = manager.get_plugin("org.jeanneproject.embeddings.generator");
    assert!(
        emb_plugin.is_some(),
        "Plugin embeddings doit être découvert"
    );
    let emb = emb_plugin.unwrap();
    assert_eq!(emb.lifecycle, PluginLifecycle::OnDemand);
    assert!(
        emb.capabilities.iter().any(|c| matches!(
            c,
            PluginCapability::EmbeddingsGenerator {
                default_dimension: 384,
                ..
            }
        )),
        "embeddings doit déclarer capability EmbeddingsGenerator avec dimension 384"
    );

    // 3. Vérification du plugin whisper
    let whisper_plugin = manager.get_plugin("org.jeanneproject.voice.whisper");
    assert!(
        whisper_plugin.is_some(),
        "Plugin voice-whisper doit être découvert"
    );
    let whisper = whisper_plugin.unwrap();
    assert!(
        whisper
            .capabilities
            .iter()
            .any(|c| matches!(c, PluginCapability::VoiceStt { .. })),
        "voice-whisper doit déclarer capability VoiceStt"
    );

    // 4. Vérification du plugin piper
    let piper_plugin = manager.get_plugin("org.jeanneproject.voice.piper");
    assert!(
        piper_plugin.is_some(),
        "Plugin voice-piper doit être découvert"
    );
    let piper = piper_plugin.unwrap();
    assert!(
        piper
            .capabilities
            .iter()
            .any(|c| matches!(c, PluginCapability::VoiceTts { .. })),
        "voice-piper doit déclarer capability VoiceTts"
    );

    // 5. Vérification du plugin pdf
    let pdf_plugin = manager.get_plugin("org.jeanneproject.parser.pdf");
    assert!(
        pdf_plugin.is_some(),
        "Plugin pdf-parser doit être découvert"
    );
    let pdf = pdf_plugin.unwrap();
    assert!(
        pdf.capabilities
            .iter()
            .any(|c| matches!(c, PluginCapability::DocumentParser { .. })),
        "pdf-parser doit déclarer capability DocumentParser"
    );
}

#[test]
fn test_find_by_capability_helpers() {
    let mut manager = PluginManager::new();
    manager.discover_plugins();

    let llm = manager.find_by_capability("llm_runner");
    assert!(llm.is_some());
    assert_eq!(llm.unwrap().id, "org.jeanneproject.llm.runner");

    let emb = manager.find_by_capability("embeddings_generator");
    assert!(emb.is_some());
    assert_eq!(emb.unwrap().id, "org.jeanneproject.embeddings.generator");

    let stt = manager.find_by_capability("voice_stt");
    assert!(stt.is_some());
    assert_eq!(stt.unwrap().id, "org.jeanneproject.voice.whisper");

    let tts = manager.find_by_capability("voice_tts");
    assert!(tts.is_some());
    assert_eq!(tts.unwrap().id, "org.jeanneproject.voice.piper");

    let pdf = manager.find_by_capability("document_parser");
    assert!(pdf.is_some());
    assert_eq!(pdf.unwrap().id, "org.jeanneproject.parser.pdf");
}

#[tokio::test]
async fn test_watchdog_timeout_on_hanging_script() {
    // Création d'un script temporaire qui ne répond jamais (sleep)
    let temp_dir = tempfile::tempdir().unwrap();
    let script_path = temp_dir.path().join("mock_hang.sh");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&script_path, "#!/bin/sh\nsleep 10\n").unwrap();
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }

    #[cfg(unix)]
    {
        let request = JsonRpcRequest::new("ping", serde_json::json!({}), 1);
        let start = std::time::Instant::now();
        let res = execute_json_rpc(
            &script_path,
            temp_dir.path(),
            &request,
            Duration::from_millis(300),
        )
        .await;

        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(1500),
            "Le watchdog doit couper rapidement"
        );
        assert!(res.is_err(), "L'exécution doit échouer par timeout");
        match res.unwrap_err() {
            jeanne_core::plugins::PluginError::Timeout(secs, _) => {
                assert_eq!(secs, 0); // 300 ms truncates to 0 secs
            }
            other => panic!("Erreur inattendue : {:?}", other),
        }
    }
}

#[tokio::test]
async fn test_embeddings_plugin_roundtrip() {
    let mut manager = PluginManager::new();
    manager.discover_plugins();

    let emb_manifest = manager
        .find_by_capability("embeddings_generator")
        .expect("Plugin embeddings doit exister");
    let executable = match ensure_plugin_executable(emb_manifest) {
        Some(exe) => exe,
        None => {
            eprintln!(
                "SKIP: Binaire jeanne-embeddings introuvable et compilateur go non disponible."
            );
            return;
        }
    };

    // 1. Test get_model_info
    let info_req = JsonRpcRequest::new("get_model_info", serde_json::json!({}), 101);
    let info_resp = execute_json_rpc(
        &executable,
        &emb_manifest.root_dir,
        &info_req,
        Duration::from_secs(5),
    )
    .await
    .expect("get_model_info doit réussir");

    let info_val = info_resp.result.expect("result doit être présent");
    assert_eq!(info_val["model_id"], "all-MiniLM-L6-v2");
    assert_eq!(info_val["dimension"], 384);
    assert_eq!(info_val["normalized"], true);

    // 2. Test embed_text
    let embed_req = JsonRpcRequest::new(
        "embed_text",
        serde_json::json!({
            "text": "Ma voiture est bleue.",
            "prompt_type": "document"
        }),
        102,
    );
    let embed_resp = execute_json_rpc(
        &executable,
        &emb_manifest.root_dir,
        &embed_req,
        Duration::from_secs(5),
    )
    .await
    .expect("embed_text doit réussir");

    let embed_val = embed_resp.result.expect("result doit être présent");
    let vec_arr = embed_val["embedding"]
        .as_array()
        .expect("embedding doit être un tableau");
    assert_eq!(vec_arr.len(), 384, "Le vecteur doit avoir dimension 384");

    // Vérification de la normalisation L2 : sum(x_i^2) ~ 1.0
    let mut sum_sq = 0.0f64;
    for item in vec_arr {
        let f = item.as_f64().unwrap();
        sum_sq += f * f;
    }
    assert!(
        (sum_sq - 1.0).abs() < 1e-4,
        "La norme L2 doit être unitaire (calculé: {})",
        sum_sq
    );
}

#[tokio::test]
async fn test_llm_runner_plugin_roundtrip() {
    let mut manager = PluginManager::new();
    manager.discover_plugins();

    let llm_manifest = manager
        .find_by_capability("llm_runner")
        .expect("Plugin llm-runner doit exister");
    let executable = match ensure_plugin_executable(llm_manifest) {
        Some(exe) => exe,
        None => {
            eprintln!(
                "SKIP: Binaire jeanne-llm-runner introuvable et compilateur go non disponible."
            );
            return;
        }
    };

    // 1. Test load_model
    let load_req = JsonRpcRequest::new(
        "load_model",
        serde_json::json!({
            "model_path": "/mock/qwen2.5-3b-instruct-q4_k_m.gguf",
            "use_gpu": true,
            "gpu_layers": 99,
            "context_size": 4096,
            "threads": 4
        }),
        201,
    );
    let load_resp = execute_json_rpc(
        &executable,
        &llm_manifest.root_dir,
        &load_req,
        Duration::from_secs(5),
    )
    .await
    .expect("load_model doit réussir");

    let load_val = load_resp.result.expect("result doit être présent");
    assert_eq!(load_val["status"], "loaded");
    assert_eq!(load_val["architecture"], "qwen2");

    // 2. Test generate_stream (requête terminée avec finish_reason)
    let gen_req = JsonRpcRequest::new(
        "generate_stream",
        serde_json::json!({
            "prompt": "Corrige ce texte : salut, commen sa va ?",
            "max_tokens": 64,
            "temperature": 0.3
        }),
        202,
    );
    let gen_resp = execute_json_rpc(
        &executable,
        &llm_manifest.root_dir,
        &gen_req,
        Duration::from_secs(5),
    )
    .await
    .expect("generate_stream doit réussir");

    let gen_val = gen_resp.result.expect("result doit être présent");
    assert_eq!(gen_val["done"], true);
    assert_eq!(gen_val["finish_reason"], "stop");
    assert!(gen_val["generated_tokens"].as_u64().unwrap() > 0);

    // 3. Test unload_model
    let unload_req = JsonRpcRequest::new("unload_model", serde_json::json!({}), 203);
    let unload_resp = execute_json_rpc(
        &executable,
        &llm_manifest.root_dir,
        &unload_req,
        Duration::from_secs(5),
    )
    .await
    .expect("unload_model doit réussir");

    let unload_val = unload_resp.result.expect("result doit être présent");
    assert_eq!(unload_val["status"], "unloaded");
    assert!(unload_val["freed_mb"].as_u64().unwrap() > 0);
}
