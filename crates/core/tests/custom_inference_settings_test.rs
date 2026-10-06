//! Test Suite for Milestone 04d: Custom Inference Server Settings & GGUF Decoupling
//!
//! Validates:
//! - TEST-04D-01: fetch_remote_models returns model list from OpenAI-compatible endpoint
//! - TEST-04D-02: fetch_remote_models handles Bearer token authentication (401 on missing/invalid)
//! - TEST-04D-03: is_inference_ready returns true when daemon_endpoint is configured even without GGUF
//! - TEST-04D-04: generate_stream executes successfully via daemon when no GGUF model is loaded
//! - TEST-04D-05: daemon uses explicit daemon_model and daemon_api_key

use jeanne_core::llm::LlmError;
use jeanne_core::local_llm::{LocalEngineConfig, LocalLlmEngine};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

async fn read_full_request(socket: &mut TcpStream) -> String {
    let mut buf = [0u8; 8192];
    let mut total = 0;
    loop {
        let n = socket.read(&mut buf[total..]).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        total += n;
        let s = String::from_utf8_lossy(&buf[..total]);
        if let Some(pos) = s.find("\r\n\r\n") {
            if s.starts_with("GET") {
                break;
            }
            if let Some(cl_idx) = s
                .find("content-length:")
                .or_else(|| s.find("Content-Length:"))
            {
                let rest = &s[cl_idx..];
                let line_end = rest.find("\r\n").unwrap_or(rest.len());
                let parts: Vec<&str> = rest[..line_end].split(':').collect();
                if parts.len() == 2 {
                    if let Ok(cl) = parts[1].trim().parse::<usize>() {
                        if total >= pos + 4 + cl {
                            break;
                        }
                    }
                }
            } else {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf[..total]).to_string()
}

#[tokio::test]
async fn test_04d_01_fetch_remote_models_success() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let _ = read_full_request(&mut socket).await;

        let body = serde_json::json!({
            "object": "list",
            "data": [
                {"id": "qwen2.5:7b-instruct-q8_0", "object": "model"},
                {"id": "mistral-nemo:12b", "object": "model"}
            ]
        })
        .to_string();

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        let _ = socket.shutdown().await;
    });

    let endpoint = format!("http://127.0.0.1:{port}/v1");
    let models = LocalLlmEngine::fetch_remote_models(&endpoint, None)
        .await
        .expect("Fetch models failed");

    assert_eq!(models.len(), 2);
    assert_eq!(models[0], "qwen2.5:7b-instruct-q8_0");
    assert_eq!(models[1], "mistral-nemo:12b");
}

#[tokio::test]
async fn test_04d_02_fetch_remote_models_with_bearer_token() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let req = read_full_request(&mut socket).await;
            if req.is_empty() {
                continue;
            }

            if !req
                .to_lowercase()
                .contains("authorization: bearer secret_token_123")
            {
                let err_body = "{\"error\": \"Unauthorized\"}";
                let resp = format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    err_body.len(),
                    err_body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            } else {
                let body = "{\"data\": [{\"id\": \"authorized-model\"}]}";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
                break;
            }
        }
    });

    let endpoint = format!("http://127.0.0.1:{port}/v1");

    // Call without token -> should fail with Auth
    let err = LocalLlmEngine::fetch_remote_models(&endpoint, None)
        .await
        .unwrap_err();
    assert_eq!(err, LlmError::Auth);

    // Call with valid token -> should succeed
    let models = LocalLlmEngine::fetch_remote_models(&endpoint, Some("secret_token_123"))
        .await
        .expect("Fetch with token failed");
    assert_eq!(models, vec!["authorized-model".to_string()]);
}

#[tokio::test]
async fn test_04d_03_is_inference_ready_local_vs_daemon() {
    let empty_config = LocalEngineConfig::default();
    let engine = LocalLlmEngine::new(empty_config);

    // No local model loaded, no daemon endpoint configured -> false
    assert!(!engine.is_inference_ready().await);

    // Now configure a daemon endpoint
    let daemon_config = LocalEngineConfig {
        daemon_endpoint: Some("http://127.0.0.1:11434/v1".to_string()),
        ..Default::default()
    };
    engine.update_config(daemon_config).await;

    // With daemon endpoint configured -> is_inference_ready should be true
    assert!(engine.is_inference_ready().await);
}

#[tokio::test]
async fn test_04d_04_stream_with_daemon_and_no_local_model() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        // Probe check on /models
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let body = "{\"data\": [{\"id\": \"remote-qwen\"}]}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(resp.as_bytes()).await;
        }

        // Second check on detect_daemon_model_name /models
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let body = "{\"data\": [{\"id\": \"remote-qwen\"}]}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(resp.as_bytes()).await;
        }

        // Third check: POST /chat/completions
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).await.unwrap();

            let sse_body = "data: {\"choices\": [{\"delta\": {\"content\": \"Bonjour \"}}]}\n\ndata: {\"choices\": [{\"delta\": {\"content\": \"monde !\"}}]}\n\ndata: [DONE]\n\n";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            let _ = socket.write_all(resp.as_bytes()).await;
        }
    });

    let config = LocalEngineConfig {
        daemon_endpoint: Some(format!("http://127.0.0.1:{port}/v1")),
        ..Default::default()
    };
    let engine = LocalLlmEngine::new(config);

    // Verify local model is NOT loaded
    assert!(!engine.is_model_loaded().await);

    // But inference is ready!
    assert!(engine.is_inference_ready().await);

    // Call generate_stream without local model loaded -> must stream from daemon!
    let cancel = CancellationToken::new();
    let mut rx = engine
        .generate_stream("Dis bonjour".to_string(), cancel)
        .await
        .expect("Stream should not fail with ModelNotLoaded");

    let mut output = String::new();
    while let Some(tok) = rx.recv().await {
        output.push_str(&tok);
    }

    assert_eq!(output, "Bonjour monde !");
}

#[tokio::test]
async fn test_04d_05_stream_uses_custom_model_and_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        // 1. Probe check on /models
        if let Ok((mut socket, _)) = listener.accept().await {
            let req = read_full_request(&mut socket).await;
            assert!(
                req.to_lowercase()
                    .contains("authorization: bearer secret_pass_456")
            );

            let body = "{\"data\": [{\"id\": \"unused-model\"}]}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(resp.as_bytes()).await.unwrap();
            let _ = socket.shutdown().await;
        }

        // 2. POST /chat/completions (detect_daemon_model_name is skipped if daemon_model is set!)
        if let Ok((mut socket, _)) = listener.accept().await {
            let req = read_full_request(&mut socket).await;

            assert!(
                req.to_lowercase()
                    .contains("authorization: bearer secret_pass_456")
            );
            // Payload must use our explicit daemon_model!
            assert!(req.contains("my-chosen-model-v2"));

            let sse_body =
                "data: {\"choices\": [{\"delta\": {\"content\": \"Succès\"}}]}\n\ndata: [DONE]\n\n";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            socket.write_all(resp.as_bytes()).await.unwrap();
            let _ = socket.shutdown().await;
        }
    });

    let config = LocalEngineConfig {
        daemon_endpoint: Some(format!("http://127.0.0.1:{port}/v1")),
        daemon_api_key: Some("secret_pass_456".to_string()),
        daemon_model: Some("my-chosen-model-v2".to_string()),
        ..Default::default()
    };
    let engine = LocalLlmEngine::new(config);

    let cancel = CancellationToken::new();
    let mut rx = engine
        .generate_stream("Test auth & model".to_string(), cancel)
        .await
        .expect("Stream execution failed");

    let mut output = String::new();
    while let Some(tok) = rx.recv().await {
        output.push_str(&tok);
    }

    assert_eq!(output, "Succès");
}
