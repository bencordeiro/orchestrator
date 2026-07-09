//! Native Anthropic Messages API adapter.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{map_http_error, Backend, ChatMessage, ChatResponse};
use crate::error::{OrchestratorError, Result};

#[derive(Clone)]
pub struct AnthropicBackend {
    client: reqwest::Client,
    base_url: String,
}

impl AnthropicBackend {
    pub fn new(client: reqwest::Client, base_url: &str) -> Self {
        Self {
            client,
            // Accept either `https://api.anthropic.com` or `.../v1`.
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn messages_url(&self) -> String {
        if self.base_url.ends_with("/v1") {
            format!("{}/messages", self.base_url)
        } else {
            format!("{}/v1/messages", self.base_url)
        }
    }
}

#[async_trait]
impl Backend for AnthropicBackend {
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        api_key: Option<&str>,
    ) -> Result<ChatResponse> {
        let api_key = api_key.ok_or_else(|| {
            OrchestratorError::WorkerUnavailable(
                "anthropic backend requires auth_ref / API key".into(),
            )
        })?;

        // Anthropic wants system as a top-level field, not in messages.
        let mut system: Option<String> = None;
        let mut api_messages = Vec::new();
        for m in messages {
            match m.role.as_str() {
                "system" => {
                    system = Some(match system.take() {
                        Some(prev) => format!("{prev}\n{}", m.content),
                        None => m.content.clone(),
                    });
                }
                "user" | "assistant" => {
                    api_messages.push(json!({
                        "role": m.role,
                        "content": m.content,
                    }));
                }
                other => {
                    api_messages.push(json!({
                        "role": "user",
                        "content": format!("[{other}] {}", m.content),
                    }));
                }
            }
        }

        if api_messages.is_empty() {
            return Err(OrchestratorError::WorkerUnavailable(
                "no user/assistant messages to send".into(),
            ));
        }

        let mut body = json!({
            "model": model,
            "messages": api_messages,
            "max_tokens": 8192,
        });
        if let Some(sys) = system {
            body["system"] = json!(sys);
        }

        let response = self
            .client
            .post(self.messages_url())
            .header("Content-Type", "application/json")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                OrchestratorError::WorkerUnavailable(format!("connection failed: {e}"))
            })?;

        let status = response.status();
        let text = response.text().await.map_err(|e| {
            OrchestratorError::WorkerUnavailable(format!("read body failed: {e}"))
        })?;

        if !status.is_success() {
            return Err(map_http_error("anthropic", status, &text));
        }

        let parsed: AnthropicResponse = serde_json::from_str(&text).map_err(|e| {
            OrchestratorError::WorkerUnavailable(format!("invalid anthropic json: {e}; body={text}"))
        })?;

        let content = parsed
            .content
            .into_iter()
            .filter(|b| b.block_type == "text")
            .map(|b| b.text.unwrap_or_default())
            .collect::<Vec<_>>()
            .join("");

        Ok(ChatResponse {
            content,
            backend_id: format!("anthropic:{}:{}", self.base_url, model),
        })
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn anthropic_happy_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "sk-ant-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "text", "text": "bonjour" }],
                "model": "claude-test",
                "stop_reason": "end_turn"
            })))
            .mount(&server)
            .await;

        let backend = AnthropicBackend::new(reqwest::Client::new(), &server.uri());
        let resp = backend
            .chat(
                "claude-test",
                &[
                    ChatMessage::system("be brief"),
                    ChatMessage::user("hello"),
                ],
                Some("sk-ant-test"),
            )
            .await
            .unwrap();
        assert_eq!(resp.content, "bonjour");
    }
}
