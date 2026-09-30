use futures_util::StreamExt;
use jeanne_core::llm::{
    ChatMessage, LlmError, LlmProvider, OpenAiClient, OpenAiConfig, build_rag_prompt,
};
use jeanne_core::models::{CoalaType, HybridSearchResult, NoteStatus};
use jeanne_core::pii::{PiiSession, PiiSlidingBuffer};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[test]
fn test_03_01_outgoing_pii_redaction() {
    let mut session = PiiSession::new();
    let text = "Contact john.doe@example.com or reach me at +33 6 12 34 56 78 immediately.";
    let masked = session.mask_text(text);

    assert!(
        masked.contains("[EMAIL_1]"),
        "Email should be masked to [EMAIL_1], got: {masked}"
    );
    assert!(
        masked.contains("[PHONE_1]"),
        "Phone should be masked to [PHONE_1], got: {masked}"
    );
    assert!(
        !masked.contains("john.doe@example.com"),
        "Original email must not leak in masked output"
    );
    assert!(
        !masked.contains("+33 6 12 34 56 78"),
        "Original phone must not leak in masked output"
    );
}

#[test]
fn test_03_02_consistent_forward_mapping() {
    let mut session = PiiSession::new();
    let text1 = "Email john.doe@example.com for info.";
    let masked1 = session.mask_text(text1);
    assert!(masked1.contains("[EMAIL_1]"));

    let text2 = "Again, write to john.doe@example.com soon.";
    let masked2 = session.mask_text(text2);
    assert!(
        masked2.contains("[EMAIL_1]"),
        "Second occurrence must reuse [EMAIL_1]"
    );
    assert_eq!(
        session.counter_email, 1,
        "Counter should not increment for existing email"
    );
}

#[test]
fn test_03_03_financial_pii_redaction() {
    let mut session = PiiSession::new();
    let text = "Wire funds to FR7630006000011234567890189 or charge card 4111-2222-3333-4444 now.";
    let masked = session.mask_text(text);

    assert!(
        masked.contains("[FINANCIAL_1]"),
        "IBAN should be masked, got: {masked}"
    );
    assert!(
        masked.contains("[FINANCIAL_2]"),
        "Card should be masked, got: {masked}"
    );
    assert!(!masked.contains("FR7630006000011234567890189"));
    assert!(!masked.contains("4111-2222-3333-4444"));
}

#[test]
fn test_03_04_complete_demasking() {
    let mut session = PiiSession::new();
    let original = "Hello alex@corp.com, call 06 12 34 56 78!";
    let masked = session.mask_text(original);
    let demasked = session.demask_text(&masked);
    assert_eq!(demasked, original);
}

#[test]
fn test_03_05_fractured_token_recovery() {
    let mut reverse_map = HashMap::new();
    reverse_map.insert("[EMAIL_1]".to_string(), "john.doe@example.com".to_string());

    let mut buffer = PiiSlidingBuffer::new(reverse_map);
    let chunks = vec!["Hello, contact [EMAIL", "_1", "] today."];

    let mut output = String::new();
    for chunk in chunks {
        output.push_str(&buffer.process_chunk(chunk));
    }
    output.push_str(&buffer.flush());

    assert_eq!(output, "Hello, contact john.doe@example.com today.");
}

#[test]
fn test_03_06_natural_bracket_preservation() {
    let reverse_map = HashMap::new();
    let mut buffer = PiiSlidingBuffer::new(reverse_map);

    let chunks = vec!["See doc ", "[chapter 3]", " and [[source: note.md]]"];
    let mut output = String::new();
    for chunk in chunks {
        output.push_str(&buffer.process_chunk(chunk));
    }
    output.push_str(&buffer.flush());

    assert_eq!(output, "See doc [chapter 3] and [[source: note.md]]");
}

#[test]
fn test_03_07_buffer_overflow_safety() {
    let reverse_map = HashMap::new();
    let mut buffer = PiiSlidingBuffer::new(reverse_map);

    let chunk = "[this is a very long string without closing bracket";
    let output = buffer.process_chunk(chunk);
    let flushed = buffer.flush();
    let combined = format!("{output}{flushed}");

    assert_eq!(combined, chunk);
}

#[test]
fn test_03_08_trailing_buffer_flush() {
    let reverse_map = HashMap::new();
    let mut buffer = PiiSlidingBuffer::new(reverse_map);

    let output = buffer.process_chunk("Truncated token [EMAIL_");
    let flushed = buffer.flush();
    let combined = format!("{output}{flushed}");

    assert_eq!(combined, "Truncated token [EMAIL_");
}

#[tokio::test]
async fn test_03_09_openai_client_request_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 2048];
        let n = socket.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();

        let sse_body =
            "data: {\"choices\": [{\"delta\": {\"content\": \"Hello\"}}]}\n\ndata: [DONE]\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            sse_body.len(),
            sse_body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        request
    });

    let config = OpenAiConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        model: "gpt-4o-mini".to_string(),
        api_key: Some("sk-test-secret-key".to_string()),
        temperature: Some(0.5),
        max_tokens: Some(100),
        timeout_secs: Some(5),
    };

    let client = OpenAiClient::new(config);
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are Jeanne.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: "Ping".to_string(),
        },
    ];

    let token = CancellationToken::new();
    let mut stream = client.chat_stream(messages, token).await.unwrap();
    let mut items = Vec::new();
    while let Some(item) = stream.next().await {
        items.push(item.unwrap());
    }

    let request_received = server_task.await.unwrap();
    assert!(request_received.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(request_received.contains("authorization: Bearer sk-test-secret-key"));
    assert!(request_received.contains("\"stream\":true"));
    assert_eq!(items, vec!["Hello"]);
}

#[tokio::test]
async fn test_03_10_sse_delta_parsing_and_done() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = socket.read(&mut buf).await.unwrap();

        let sse_body = "data: {\"choices\": [{\"delta\": {\"content\": \"First \"}}]}\n\ndata: {\"choices\": [{\"delta\": {\"content\": \"Second\"}}]}\n\ndata: [DONE]\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            sse_body.len(),
            sse_body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    });

    let config = OpenAiConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        model: "gpt-4o-mini".to_string(),
        api_key: None,
        ..Default::default()
    };
    let client = OpenAiClient::new(config);
    let token = CancellationToken::new();
    let mut stream = client
        .chat_stream(
            vec![ChatMessage {
                role: "user".to_string(),
                content: "Hi".to_string(),
            }],
            token,
        )
        .await
        .unwrap();

    let mut full_output = String::new();
    while let Some(chunk) = stream.next().await {
        full_output.push_str(&chunk.unwrap());
    }

    assert_eq!(full_output, "First Second");
}

#[tokio::test]
async fn test_03_11_immediate_cancellation() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = socket.read(&mut buf).await.unwrap();

        let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
        socket.write_all(header.as_bytes()).await.unwrap();

        for i in 0..100 {
            let chunk_data = format!(
                "data: {{\"choices\": [{{\"delta\": {{\"content\": \"token{i} \"}}}}]}}\n\n"
            );
            let chunk_http = format!("{:X}\r\n{}\r\n", chunk_data.len(), chunk_data);
            if socket.write_all(chunk_http.as_bytes()).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    let config = OpenAiConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        ..Default::default()
    };
    let client = OpenAiClient::new(config);
    let token = CancellationToken::new();
    let token_clone = token.clone();

    let mut stream = client
        .chat_stream(
            vec![ChatMessage {
                role: "user".to_string(),
                content: "Stream".to_string(),
            }],
            token,
        )
        .await
        .unwrap();

    // Consume first token, then cancel
    let first = stream.next().await;
    assert!(first.is_some());

    let start = std::time::Instant::now();
    token_clone.cancel();

    let next_chunk = stream.next().await;
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(100),
        "Cancellation took too long: {elapsed:?}"
    );

    match next_chunk {
        Some(Err(LlmError::Cancelled)) | None => {}
        other => panic!("Expected Cancelled error or clean termination, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_03_12_api_error_mapping() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        // First connection: 401 Unauthorized
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let body = "{\"error\": {\"message\": \"Invalid API key\"}}";
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }

        // Second connection: 429 Too Many Requests
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let body = "{\"error\": {\"message\": \"Rate limit exceeded\"}}";
            let response = format!(
                "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });

    let config = OpenAiConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        ..Default::default()
    };
    let client = OpenAiClient::new(config);

    // Call 1 -> 401 Auth
    let res1 = client
        .chat_stream(
            vec![ChatMessage {
                role: "user".to_string(),
                content: "1".to_string(),
            }],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(res1.err(), Some(LlmError::Auth));

    // Call 2 -> 429 Api
    let res2 = client
        .chat_stream(
            vec![ChatMessage {
                role: "user".to_string(),
                content: "2".to_string(),
            }],
            CancellationToken::new(),
        )
        .await;
    match res2 {
        Err(LlmError::Api { status, .. }) => assert_eq!(status, 429),
        Err(e) => panic!("Expected Api error with status 429, got err: {e:?}"),
        Ok(_) => panic!("Expected Err with status 429, but got Ok stream"),
    }
}

#[test]
fn test_03_13_rag_prompt_builder_and_citations() {
    let contexts = vec![
        HybridSearchResult {
            chunk_id: "specs:0".to_string(),
            file_path: "Ressources/specs.md".to_string(),
            content: "Max RAM allocation is 150 MB for streaming.".to_string(),
            vector_score: 0.9,
            bm25_score: 0.8,
            combined_score: 0.87,
            coala_type: CoalaType::Semantic,
            status: NoteStatus::Active,
            superseded_by: None,
            deprecated_at: None,
            age_days: 1.0,
        },
        HybridSearchResult {
            chunk_id: "arch:0".to_string(),
            file_path: "Casquettes/architecte.md".to_string(),
            content: "Spec-Driven Development is mandatory.".to_string(),
            vector_score: 0.85,
            bm25_score: 0.7,
            combined_score: 0.81,
            coala_type: CoalaType::Procedural,
            status: NoteStatus::Active,
            superseded_by: None,
            deprecated_at: None,
            age_days: 0.0,
        },
    ];

    let messages = build_rag_prompt("What are the RAM rules?", &contexts, None);

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "system");
    assert!(messages[0].content.contains("[source: filename.md]"));

    assert_eq!(messages[1].role, "user");
    assert!(
        messages[1]
            .content
            .contains("[source: Ressources/specs.md]")
    );
    assert!(
        messages[1]
            .content
            .contains("Max RAM allocation is 150 MB for streaming.")
    );
    assert!(
        messages[1]
            .content
            .contains("[source: Casquettes/architecte.md]")
    );
    assert!(messages[1].content.contains("What are the RAM rules?"));
}

#[tokio::test]
async fn test_03_14_end_to_end_pii_masked_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 2048];
        let n = socket.read(&mut buf).await.unwrap();
        let request_body = String::from_utf8_lossy(&buf[..n]).to_string();

        // Model responds referring to the masked token
        let sse_body = "data: {\"choices\": [{\"delta\": {\"content\": \"Noted! I will email [EMAIL\"}}]}\n\ndata: {\"choices\": [{\"delta\": {\"content\": \"_1] shortly.\"}}]}\n\ndata: [DONE]\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            sse_body.len(),
            sse_body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        request_body
    });

    let mut session = PiiSession::new();
    let user_prompt = "My secret email is secret_agent@jeanne.ai.";
    let masked_prompt = session.mask_text(user_prompt);

    let config = OpenAiConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        ..Default::default()
    };
    let client = OpenAiClient::new(config);

    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: masked_prompt,
    }];

    let token = CancellationToken::new();
    let mut raw_stream = client.chat_stream(messages, token).await.unwrap();

    let mut sliding_buffer = PiiSlidingBuffer::new(session.reverse_map.clone());
    let mut client_demasked_output = String::new();

    while let Some(chunk_res) = raw_stream.next().await {
        let chunk = chunk_res.unwrap();
        client_demasked_output.push_str(&sliding_buffer.process_chunk(&chunk));
    }
    client_demasked_output.push_str(&sliding_buffer.flush());

    let wire_request = server_task.await.unwrap();

    // 1. Verify wire payload never contained secret_agent@jeanne.ai
    assert!(
        !wire_request.contains("secret_agent@jeanne.ai"),
        "Wire payload must NOT contain raw PII"
    );
    assert!(
        wire_request.contains("[EMAIL_1]"),
        "Wire payload must contain masked placeholder [EMAIL_1]"
    );

    // 2. Verify client end-to-end output restored the raw email
    assert_eq!(
        client_demasked_output,
        "Noted! I will email secret_agent@jeanne.ai shortly."
    );
}
