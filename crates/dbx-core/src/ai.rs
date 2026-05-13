use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock};
use tokio::sync::{Notify, RwLock};

// ---------------------------------------------------------------------------
// Stream cancel registry
// ---------------------------------------------------------------------------

static AI_STREAMS: LazyLock<RwLock<HashMap<String, Arc<Notify>>>> = LazyLock::new(|| RwLock::new(HashMap::new()));

pub async fn register_stream(session_id: &str) -> Arc<Notify> {
    let notify = Arc::new(Notify::new());
    AI_STREAMS.write().await.insert(session_id.to_string(), notify.clone());
    notify
}

pub async fn cancel_stream(session_id: &str) -> bool {
    if let Some(notify) = AI_STREAMS.read().await.get(session_id) {
        notify.notify_one();
        true
    } else {
        false
    }
}

pub async fn unregister_stream(session_id: &str) {
    AI_STREAMS.write().await.remove(session_id);
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    Claude,
    Openai,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AiApiStyle {
    Completions,
    Responses,
}

impl Default for AiApiStyle {
    fn default() -> Self {
        Self::Completions
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub provider: AiProvider,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_style: AiApiStyle,
    #[serde(default)]
    pub proxy_enabled: bool,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default = "default_enable_thinking")]
    pub enable_thinking: bool,
}

fn default_enable_thinking() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCompletionRequest {
    pub config: AiConfig,
    pub system_prompt: String,
    pub messages: Vec<AiMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiStreamChunk {
    pub session_id: String,
    pub delta: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_delta: Option<String>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConversation {
    pub id: String,
    pub title: String,
    pub connection_name: String,
    pub database: String,
    pub messages: Vec<AiChatMessage>,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

pub fn resolve_endpoint(config: &AiConfig) -> String {
    let ep = config.endpoint.trim().trim_end_matches('/');
    if ep.ends_with("/chat/completions") || ep.ends_with("/responses") || ep.ends_with("/messages") {
        return ep.to_string();
    }
    match config.provider {
        AiProvider::Claude => format!("{ep}/messages"),
        AiProvider::Openai | AiProvider::Custom => {
            if config.api_style == AiApiStyle::Responses {
                format!("{ep}/responses")
            } else {
                format!("{ep}/chat/completions")
            }
        }
    }
}

pub fn stream_data_payload(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') || line.starts_with("event:") || line.starts_with("id:") {
        return None;
    }
    if let Some(data) = line.strip_prefix("data:") {
        return Some(data.trim_start());
    }
    if line.starts_with('{') {
        return Some(line);
    }
    None
}

pub fn claude_stream_text(event: &serde_json::Value) -> Option<&str> {
    if event["type"] == "content_block_delta" {
        return event["delta"]["text"].as_str();
    }
    None
}

pub fn openai_stream_text(event: &serde_json::Value) -> Option<&str> {
    event["choices"]
        .get(0)
        .and_then(|choice| choice["delta"]["content"].as_str().or_else(|| choice["message"]["content"].as_str()))
        .or_else(|| event["content"].as_str())
        .filter(|text| !text.is_empty())
}

pub fn openai_stream_reasoning(event: &serde_json::Value) -> Option<&str> {
    event["choices"]
        .get(0)
        .and_then(|choice| choice["delta"]["reasoning_content"].as_str())
        .filter(|text| !text.is_empty())
}

pub fn responses_stream_text(event: &serde_json::Value) -> Option<&str> {
    event["delta"].as_str().filter(|s| !s.is_empty())
}

pub fn extract_error(data: &serde_json::Value) -> Option<String> {
    data["error"]["message"].as_str().or_else(|| data["error"].as_str()).map(ToString::to_string)
}

pub fn build_responses_input(system_prompt: &str, messages: &[AiMessage]) -> serde_json::Value {
    let mut input = Vec::new();
    if !system_prompt.is_empty() {
        input.push(json!({
            "role": "developer",
            "content": system_prompt,
        }));
    }
    for m in messages {
        input.push(json!({
            "role": m.role,
            "content": m.content,
        }));
    }
    json!(input)
}

// ---------------------------------------------------------------------------
// Validation helper
// ---------------------------------------------------------------------------

fn validate_config(config: &AiConfig) -> Result<(), String> {
    if config.api_key.trim().is_empty() {
        return Err("API key is required".to_string());
    }
    if config.endpoint.trim().is_empty() {
        return Err("Endpoint is required".to_string());
    }
    if config.model.trim().is_empty() {
        return Err("Model is required".to_string());
    }
    Ok(())
}

pub fn build_ai_http_client(config: &AiConfig, timeout_secs: u64) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(timeout_secs));
    if config.proxy_enabled && !config.proxy_url.trim().is_empty() {
        let proxy = reqwest::Proxy::all(config.proxy_url.trim()).map_err(|e| format!("Invalid AI proxy URL: {e}"))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Non-streaming calls
// ---------------------------------------------------------------------------

pub async fn call_claude(client: &reqwest::Client, request: AiCompletionRequest) -> Result<String, String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&request.config.api_key).map_err(|e| e.to_string())?);
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

    let body = json!({
        "model": request.config.model,
        "max_tokens": request.max_tokens.unwrap_or(2048),
        "temperature": request.temperature.unwrap_or(0.2),
        "system": request.system_prompt,
        "messages": request.messages,
    });

    let res = client
        .post(&resolve_endpoint(&request.config))
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Claude request failed: {e}"))?;

    let status = res.status();
    let data: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(extract_error(&data).unwrap_or_else(|| format!("Claude API error: {status}")));
    }

    Ok(data["content"]
        .as_array()
        .and_then(|items| items.iter().find_map(|item| item["text"].as_str()))
        .unwrap_or_default()
        .to_string())
}

pub async fn call_openai_compatible(client: &reqwest::Client, request: AiCompletionRequest) -> Result<String, String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", request.config.api_key)).map_err(|e| e.to_string())?,
    );

    let mut messages = vec![json!({ "role": "system", "content": request.system_prompt })];
    messages.extend(request.messages.iter().map(|message| json!({ "role": message.role, "content": message.content })));

    let mut body_obj = json!({
        "model": request.config.model,
        "messages": messages,
        "max_tokens": request.max_tokens.unwrap_or(2048),
        "temperature": request.temperature.unwrap_or(0.2),
    });
    if !request.config.enable_thinking {
        body_obj["extra_body"] = json!({
            "chat_template_kwargs": { "enable_thinking": false }
        });
    }

    let res = client
        .post(&resolve_endpoint(&request.config))
        .headers(headers)
        .json(&body_obj)
        .send()
        .await
        .map_err(|e| format!("AI request failed: {e}"))?;

    let status = res.status();
    let data: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(extract_error(&data).unwrap_or_else(|| format!("API error: {status}")));
    }

    Ok(data["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string())
}

pub async fn call_responses_api(client: &reqwest::Client, request: AiCompletionRequest) -> Result<String, String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", request.config.api_key)).map_err(|e| e.to_string())?,
    );

    let body = json!({
        "model": request.config.model,
        "input": build_responses_input(&request.system_prompt, &request.messages),
        "max_output_tokens": request.max_tokens.unwrap_or(2048),
        "temperature": request.temperature.unwrap_or(0.2),
    });

    let res = client
        .post(&resolve_endpoint(&request.config))
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("AI request failed: {e}"))?;

    let status = res.status();
    let data: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(extract_error(&data).unwrap_or_else(|| format!("API error: {status}")));
    }

    Ok(data["output"]
        .as_array()
        .and_then(|items| {
            items.iter().find_map(|item| {
                item["content"].as_array().and_then(|parts| parts.iter().find_map(|p| p["text"].as_str()))
            })
        })
        .unwrap_or_default()
        .to_string())
}

// ---------------------------------------------------------------------------
// High-level: test_connection_core / complete
// ---------------------------------------------------------------------------

pub async fn test_connection_core(config: &AiConfig) -> Result<String, String> {
    validate_config(config)?;

    let client = build_ai_http_client(config, 15)?;

    let request = AiCompletionRequest {
        config: config.clone(),
        system_prompt: String::new(),
        messages: vec![AiMessage { role: "user".into(), content: "hi".into() }],
        max_tokens: Some(1),
        temperature: Some(0.0),
    };

    match request.config.provider {
        AiProvider::Claude => call_claude(&client, request).await,
        AiProvider::Openai | AiProvider::Custom => {
            if request.config.api_style == AiApiStyle::Responses {
                call_responses_api(&client, request).await
            } else {
                call_openai_compatible(&client, request).await
            }
        }
    }
    .map(|_| "OK".to_string())
}

pub async fn complete(request: &AiCompletionRequest) -> Result<String, String> {
    validate_config(&request.config)?;

    let client = build_ai_http_client(&request.config, 60)?;

    match request.config.provider {
        AiProvider::Claude => call_claude(&client, request.clone()).await,
        AiProvider::Openai | AiProvider::Custom => {
            if request.config.api_style == AiApiStyle::Responses {
                call_responses_api(&client, request.clone()).await
            } else {
                call_openai_compatible(&client, request.clone()).await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

pub async fn stream(
    session_id: &str,
    request: &AiCompletionRequest,
    cancelled: &Notify,
    on_chunk: impl Fn(AiStreamChunk),
) -> Result<(), String> {
    validate_config(&request.config)?;

    let client = build_ai_http_client(&request.config, 120)?;

    match request.config.provider {
        AiProvider::Claude => stream_claude(&client, session_id, request, cancelled, &on_chunk).await,
        AiProvider::Openai | AiProvider::Custom => {
            if request.config.api_style == AiApiStyle::Responses {
                stream_responses_api(&client, session_id, request, cancelled, &on_chunk).await
            } else {
                stream_openai(&client, session_id, request, cancelled, &on_chunk).await
            }
        }
    }
}

async fn stream_claude(
    client: &reqwest::Client,
    session_id: &str,
    request: &AiCompletionRequest,
    cancelled: &Notify,
    on_chunk: &impl Fn(AiStreamChunk),
) -> Result<(), String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("x-api-key", HeaderValue::from_str(&request.config.api_key).map_err(|e| e.to_string())?);
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

    let body = json!({
        "model": request.config.model,
        "max_tokens": request.max_tokens.unwrap_or(2048),
        "temperature": request.temperature.unwrap_or(0.2),
        "system": request.system_prompt,
        "messages": request.messages,
        "stream": true,
    });

    let res = client
        .post(&resolve_endpoint(&request.config))
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Claude request failed: {e}"))?;

    if !res.status().is_success() {
        let data: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
        return Err(extract_error(&data).unwrap_or_else(|| "Claude API error".to_string()));
    }

    let mut byte_stream = res.bytes_stream();
    let mut buf = String::new();

    loop {
        tokio::select! {
            chunk = byte_stream.next() => {
                let Some(chunk) = chunk else { break };
                let chunk = chunk.map_err(|e| e.to_string())?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                let mut finished = false;
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].to_string();
                    buf = buf[pos + 1..].to_string();

                    let Some(data) = stream_data_payload(&line) else { continue };
                    if data == "[DONE]" {
                        finished = true;
                        break;
                    }

                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(text) = claude_stream_text(&event) {
                            on_chunk(AiStreamChunk {
                                session_id: session_id.to_string(),
                                delta: text.to_string(),
                                reasoning_delta: None,
                                done: false,
                            });
                        }
                    }
                }

                if finished { break; }
            }
            _ = cancelled.notified() => { break; }
        }
    }

    on_chunk(AiStreamChunk {
        session_id: session_id.to_string(),
        delta: String::new(),
        reasoning_delta: None,
        done: true,
    });

    Ok(())
}

async fn stream_openai(
    client: &reqwest::Client,
    session_id: &str,
    request: &AiCompletionRequest,
    cancelled: &Notify,
    on_chunk: &impl Fn(AiStreamChunk),
) -> Result<(), String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", request.config.api_key)).map_err(|e| e.to_string())?,
    );

    let mut messages = vec![json!({ "role": "system", "content": request.system_prompt })];
    messages.extend(request.messages.iter().map(|m| json!({ "role": m.role, "content": m.content })));

    let mut body_obj = json!({
        "model": request.config.model,
        "messages": messages,
        "max_tokens": request.max_tokens.unwrap_or(2048),
        "temperature": request.temperature.unwrap_or(0.2),
        "stream": true,
    });
    if !request.config.enable_thinking {
        body_obj["extra_body"] = json!({
            "chat_template_kwargs": { "enable_thinking": false }
        });
    }

    let res = client
        .post(&resolve_endpoint(&request.config))
        .headers(headers)
        .json(&body_obj)
        .send()
        .await
        .map_err(|e| format!("AI request failed: {e}"))?;

    if !res.status().is_success() {
        let data: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
        return Err(extract_error(&data).unwrap_or_else(|| "API error".to_string()));
    }

    let mut byte_stream = res.bytes_stream();
    let mut buf = String::new();

    loop {
        tokio::select! {
            chunk = byte_stream.next() => {
                let Some(chunk) = chunk else { break };
                let chunk = chunk.map_err(|e| e.to_string())?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                let mut finished = false;
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].to_string();
                    buf = buf[pos + 1..].to_string();

                    let Some(data) = stream_data_payload(&line) else { continue };
                    if data == "[DONE]" {
                        finished = true;
                        break;
                    }

                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(reasoning) = openai_stream_reasoning(&event) {
                            on_chunk(AiStreamChunk {
                                session_id: session_id.to_string(),
                                delta: String::new(),
                                reasoning_delta: Some(reasoning.to_string()),
                                done: false,
                            });
                        }
                        if let Some(text) = openai_stream_text(&event) {
                            on_chunk(AiStreamChunk {
                                session_id: session_id.to_string(),
                                delta: text.to_string(),
                                reasoning_delta: None,
                                done: false,
                            });
                        }
                    }
                }

                if finished { break; }
            }
            _ = cancelled.notified() => { break; }
        }
    }

    on_chunk(AiStreamChunk {
        session_id: session_id.to_string(),
        delta: String::new(),
        reasoning_delta: None,
        done: true,
    });

    Ok(())
}

async fn stream_responses_api(
    client: &reqwest::Client,
    session_id: &str,
    request: &AiCompletionRequest,
    cancelled: &Notify,
    on_chunk: &impl Fn(AiStreamChunk),
) -> Result<(), String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", request.config.api_key)).map_err(|e| e.to_string())?,
    );

    let body = json!({
        "model": request.config.model,
        "input": build_responses_input(&request.system_prompt, &request.messages),
        "max_output_tokens": request.max_tokens.unwrap_or(2048),
        "temperature": request.temperature.unwrap_or(0.2),
        "stream": true,
    });

    let res = client
        .post(&resolve_endpoint(&request.config))
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("AI request failed: {e}"))?;

    if !res.status().is_success() {
        let data: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
        return Err(extract_error(&data).unwrap_or_else(|| "API error".to_string()));
    }

    let mut byte_stream = res.bytes_stream();
    let mut buf = String::new();

    loop {
        tokio::select! {
            chunk = byte_stream.next() => {
                let Some(chunk) = chunk else { break };
                let chunk = chunk.map_err(|e| e.to_string())?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                let mut finished = false;
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].to_string();
                    buf = buf[pos + 1..].to_string();

                    let Some(data) = stream_data_payload(&line) else { continue };
                    if data == "[DONE]" {
                        finished = true;
                        break;
                    }

                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(text) = responses_stream_text(&event) {
                            on_chunk(AiStreamChunk {
                                session_id: session_id.to_string(),
                                delta: text.to_string(),
                                reasoning_delta: None,
                                done: false,
                            });
                        }
                    }
                }

                if finished { break; }
            }
            _ = cancelled.notified() => { break; }
        }
    }

    on_chunk(AiStreamChunk {
        session_id: session_id.to_string(),
        delta: String::new(),
        reasoning_delta: None,
        done: true,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Conversation persistence (path-based)
// ---------------------------------------------------------------------------

const MAX_CONVERSATIONS: usize = 50;

pub fn read_conversations(path: &Path) -> Result<Vec<AiConversation>, String> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

pub fn write_conversations(path: &Path, conversations: &[AiConversation]) -> Result<(), String> {
    let json = serde_json::to_string(conversations).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn save_conversation(path: &Path, conversation: AiConversation) -> Result<(), String> {
    let mut conversations = read_conversations(path)?;
    if let Some(pos) = conversations.iter().position(|c| c.id == conversation.id) {
        conversations[pos] = conversation;
    } else {
        conversations.insert(0, conversation);
        conversations.truncate(MAX_CONVERSATIONS);
    }
    write_conversations(path, &conversations)
}

pub fn load_conversations(path: &Path) -> Result<Vec<AiConversation>, String> {
    read_conversations(path)
}

pub fn delete_conversation(path: &Path, id: &str) -> Result<(), String> {
    let conversations: Vec<AiConversation> = read_conversations(path)?.into_iter().filter(|c| c.id != id).collect();
    write_conversations(path, &conversations)
}

pub fn save_config(path: &Path, config: &AiConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn load_config(path: &Path) -> Result<Option<AiConfig>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map(Some).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_ai_http_client, AiApiStyle, AiConfig, AiProvider};

    #[test]
    fn ai_config_proxy_fields_default_for_legacy_config() {
        let config: AiConfig = serde_json::from_value(serde_json::json!({
            "provider": "openai",
            "apiKey": "key",
            "endpoint": "https://api.openai.com/v1/chat/completions",
            "model": "gpt-4o",
            "apiStyle": "completions"
        }))
        .unwrap();

        assert_eq!(config.proxy_enabled, false);
        assert_eq!(config.proxy_url, "");
        assert_eq!(config.enable_thinking, true);
    }

    #[test]
    fn ai_http_client_rejects_invalid_proxy_url() {
        let config = AiConfig {
            provider: AiProvider::Openai,
            api_key: "key".to_string(),
            endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
            model: "gpt-4o".to_string(),
            api_style: AiApiStyle::Completions,
            proxy_enabled: true,
            proxy_url: "not a proxy url".to_string(),
            enable_thinking: true,
        };

        let err = build_ai_http_client(&config, 1).unwrap_err();

        assert!(err.contains("Invalid AI proxy URL"));
    }
}
