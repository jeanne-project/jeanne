use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::time::Duration;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("API error status {status}: {message}")]
    Api { status: u16, message: String },
    #[error("Authentication failed or API key missing")]
    Auth,
    #[error("Inference cancelled by user")]
    Cancelled,
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Keyring error: {0}")]
    Keyring(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Stream error: {0}")]
    Stream(String),
}

impl PartialEq for LlmError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Api {
                    status: s1,
                    message: m1,
                },
                Self::Api {
                    status: s2,
                    message: m2,
                },
            ) => s1 == s2 && m1 == m2,
            (Self::Auth, Self::Auth) => true,
            (Self::Cancelled, Self::Cancelled) => true,
            (Self::Keyring(s1), Self::Keyring(s2)) => s1 == s2,
            (Self::Config(s1), Self::Config(s2)) => s1 == s2,
            (Self::Stream(s1), Self::Stream(s2)) => s1 == s2,
            (Self::Serialization(e1), Self::Serialization(e2)) => e1.to_string() == e2.to_string(),
            (Self::Network(e1), Self::Network(e2)) => e1.to_string() == e2.to_string(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout_secs: Option<u64>,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_key: None,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            timeout_secs: Some(30),
        }
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        cancellation_token: CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>, LlmError>;

    async fn health_check(&self) -> Result<bool, LlmError>;
    async fn fetch_models(&self) -> Result<Vec<String>, LlmError>;
}

pub struct OpenAiClient {
    client: reqwest::Client,
    config: OpenAiConfig,
}

impl OpenAiClient {
    pub fn new(config: OpenAiConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub fn config(&self) -> &OpenAiConfig {
        &self.config
    }
}

#[derive(Serialize)]
struct ChatCompletionsPayload<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct StreamResponse {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

#[async_trait]
impl LlmProvider for OpenAiClient {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        cancellation_token: CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>, LlmError> {
        let endpoint = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let payload = ChatCompletionsPayload {
            model: &self.config.model,
            messages: &messages,
            stream: true,
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };

        let mut request = self.client.post(&endpoint).json(&payload);

        if let Some(key) = &self.config.api_key {
            request = request.bearer_auth(key);
        }

        if let Some(timeout_secs) = self.config.timeout_secs {
            request = request.timeout(Duration::from_secs(timeout_secs));
        }

        let response = match request.send().await {
            Ok(resp) => resp,
            Err(err) => return Err(LlmError::Network(err)),
        };

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(LlmError::Auth);
        }

        if !status.is_success() {
            let status_code = status.as_u16();
            let body = match response.text().await {
                Ok(text) => text,
                Err(err) => format!("Could not read response body: {err}"),
            };
            return Err(LlmError::Api {
                status: status_code,
                message: body,
            });
        }

        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            let mut event_stream = response.bytes_stream().eventsource();

            loop {
                tokio::select! {
                    biased;
                    _ = cancellation_token.cancelled() => {
                        let _ = tx.send(Err(LlmError::Cancelled)).await;
                        break;
                    }
                    event_res = event_stream.next() => {
                        match event_res {
                            Some(Ok(event)) => {
                                let trimmed = event.data.trim();
                                if trimmed == "[DONE]" {
                                    break;
                                }

                                if let Ok(parsed) = serde_json::from_str::<StreamResponse>(trimmed) {
                                    for choice in parsed.choices {
                                        if let Some(content) = choice.delta.content {
                                            if !content.is_empty()
                                                && tx.send(Ok(content)).await.is_err()
                                            {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                            Some(Err(err)) => {
                                let _ = tx.send(Err(LlmError::Stream(err.to_string()))).await;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });

        Ok(Box::pin(stream))
    }

    async fn health_check(&self) -> Result<bool, LlmError> {
        let endpoint = format!("{}/models", self.config.base_url.trim_end_matches('/'));
        let mut req = self.client.get(&endpoint);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        if let Some(timeout_secs) = self.config.timeout_secs {
            req = req.timeout(Duration::from_secs(timeout_secs));
        }

        match req.send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(err) => Err(LlmError::Network(err)),
        }
    }

    async fn fetch_models(&self) -> Result<Vec<String>, LlmError> {
        let endpoint = format!("{}/models", self.config.base_url.trim_end_matches('/'));
        let mut req = self.client.get(&endpoint);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        if let Some(timeout_secs) = self.config.timeout_secs {
            req = req.timeout(Duration::from_secs(timeout_secs));
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(err) => return Err(LlmError::Network(err)),
        };

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = match resp.text().await {
                Ok(t) => t,
                Err(e) => format!("Error reading body: {e}"),
            };
            return Err(LlmError::Api {
                status,
                message: body,
            });
        }

        let models_resp = match resp.json::<ModelsResponse>().await {
            Ok(m) => m,
            Err(err) => return Err(LlmError::Network(err)),
        };

        Ok(models_resp.data.into_iter().map(|m| m.id).collect())
    }
}

pub struct KeyringManager;

impl KeyringManager {
    pub fn set_api_key(service: &str, user: &str, key: &str) -> Result<(), LlmError> {
        let entry = match keyring::Entry::new(service, user) {
            Ok(e) => e,
            Err(err) => return Err(LlmError::Keyring(err.to_string())),
        };
        match entry.set_password(key) {
            Ok(()) => Ok(()),
            Err(err) => Err(LlmError::Keyring(err.to_string())),
        }
    }

    pub fn get_api_key(service: &str, user: &str) -> Result<Option<String>, LlmError> {
        let entry = match keyring::Entry::new(service, user) {
            Ok(e) => e,
            Err(err) => return Err(LlmError::Keyring(err.to_string())),
        };
        match entry.get_password() {
            Ok(pwd) => Ok(Some(pwd)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(LlmError::Keyring(err.to_string())),
        }
    }

    pub fn delete_api_key(service: &str, user: &str) -> Result<(), LlmError> {
        let entry = match keyring::Entry::new(service, user) {
            Ok(e) => e,
            Err(err) => return Err(LlmError::Keyring(err.to_string())),
        };
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(LlmError::Keyring(err.to_string())),
        }
    }
}

/// Construit le prompt avec injection des documents de contexte et obligation de citation de source
/// au format strict `[source: nom_note.md]`.
pub fn build_rag_prompt(
    user_query: &str,
    context_results: &[crate::models::HybridSearchResult],
    custom_system_prompt: Option<&str>,
) -> Vec<ChatMessage> {
    let system_instructions = custom_system_prompt.unwrap_or(
        "You are Jeanne, a privacy-first AI knowledge assistant. Answer the user's question using ONLY the provided document contexts. Every factual claim MUST cite its source note using the format `[source: filename.md]`."
    );

    let mut context_block = String::new();
    for res in context_results {
        context_block.push_str(&format!("[source: {}]\n{}\n\n", res.file_path, res.content));
    }

    let user_content = if context_block.is_empty() {
        user_query.to_string()
    } else {
        format!(
            "Context Documents:\n{}\nQuestion: {}",
            context_block.trim_end(),
            user_query
        )
    };

    vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_instructions.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_content,
        },
    ]
}
