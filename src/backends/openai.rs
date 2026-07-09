//! OpenAI-compatible chat-completions adapter (streaming).
//!
//! Covers CLIProxyAPI, Ollama, llama.cpp servers, and most commercial proxies.

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;

use super::{map_http_error, Backend, ChatMessage, ChatResponse};
use crate::error::{OrchestratorError, Result};

#[derive(Clone)]
pub struct OpenAiCompatibleBackend {
    client: reqwest::Client,
    base_url: String,
}

impl OpenAiCompatibleBackend {
    pub fn new(client: reqwest::Client, base_url: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[async_trait]
impl Backend for OpenAiCompatibleBackend {
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        api_key: Option<&str>,
    ) -> Result<ChatResponse> {
        // Prefer streaming; fall back to non-stream if the body is not SSE.
        match self.chat_streaming(model, messages, api_key).await {
            Ok(resp) => Ok(resp),
            Err(OrchestratorError::WorkerUnavailable(ref reason))
                if reason.contains("non-sse") || reason.contains("stream unsupported") =>
            {
                self.chat_non_streaming(model, messages, api_key).await
            }
            Err(e) => Err(e),
        }
    }
}

impl OpenAiCompatibleBackend {
    async fn chat_streaming(
        &self,
        model: &str,
        messages: &[ChatMessage],
        api_key: Option<&str>,
    ) -> Result<ChatResponse> {
        let url = self.chat_url();
        let body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
        });

        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(key) = api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await.map_err(|e| {
            OrchestratorError::WorkerUnavailable(format!("connection failed: {e}"))
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(map_http_error("openai-compatible stream", status, &body));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !content_type.contains("text/event-stream")
            && !content_type.contains("application/x-ndjson")
        {
            // Some servers ignore stream:true and return a full JSON body.
            let text = response.text().await.map_err(|e| {
                OrchestratorError::WorkerUnavailable(format!("read body failed: {e}"))
            })?;
            if let Ok(parsed) = serde_json::from_str::<NonStreamResponse>(&text) {
                let content = parsed
                    .choices
                    .first()
                    .and_then(|c| c.message.as_ref())
                    .map(|m| m.content.clone())
                    .unwrap_or_default();
                return Ok(ChatResponse {
                    content,
                    backend_id: format!("openai_compatible:{}:{}", self.base_url, model),
                });
            }
            return Err(OrchestratorError::WorkerUnavailable(format!(
                "non-sse response (content-type={content_type}): {text}"
            )));
        }

        let mut stream = response.bytes_stream().eventsource();
        let mut content = String::new();

        while let Some(event) = stream.next().await {
            let event = event.map_err(|e| {
                OrchestratorError::WorkerUnavailable(format!("stream read failed: {e}"))
            })?;
            let data = event.data.trim();
            if data.is_empty() || data == "[DONE]" {
                if data == "[DONE]" {
                    break;
                }
                continue;
            }
            let chunk: StreamChunk = serde_json::from_str(data).map_err(|e| {
                OrchestratorError::WorkerUnavailable(format!(
                    "invalid stream chunk: {e}; data={data}"
                ))
            })?;
            for choice in chunk.choices {
                if let Some(delta) = choice.delta {
                    if let Some(piece) = delta.content {
                        content.push_str(&piece);
                    }
                }
            }
        }

        Ok(ChatResponse {
            content,
            backend_id: format!("openai_compatible:{}:{}", self.base_url, model),
        })
    }

    async fn chat_non_streaming(
        &self,
        model: &str,
        messages: &[ChatMessage],
        api_key: Option<&str>,
    ) -> Result<ChatResponse> {
        let url = self.chat_url();
        let body = json!({
            "model": model,
            "messages": messages,
            "stream": false,
        });

        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(key) = api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await.map_err(|e| {
            OrchestratorError::WorkerUnavailable(format!("connection failed: {e}"))
        })?;

        let status = response.status();
        let text = response.text().await.map_err(|e| {
            OrchestratorError::WorkerUnavailable(format!("read body failed: {e}"))
        })?;

        if !status.is_success() {
            return Err(map_http_error("openai-compatible", status, &text));
        }

        let parsed: NonStreamResponse = serde_json::from_str(&text).map_err(|e| {
            OrchestratorError::WorkerUnavailable(format!("invalid response json: {e}; body={text}"))
        })?;

        let content = parsed
            .choices
            .first()
            .and_then(|c| c.message.as_ref())
            .map(|m| m.content.clone())
            .unwrap_or_default();

        Ok(ChatResponse {
            content,
            backend_id: format!("openai_compatible:{}:{}", self.base_url, model),
        })
    }
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<Delta>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NonStreamResponse {
    #[serde(default)]
    choices: Vec<NonStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct NonStreamChoice {
    message: Option<NonStreamMessage>,
}

#[derive(Debug, Deserialize)]
struct NonStreamMessage {
    #[serde(default)]
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn non_stream_openai_compatible() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-1",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "hello from mock" },
                    "finish_reason": "stop"
                }]
            })))
            .mount(&server)
            .await;

        let backend =
            OpenAiCompatibleBackend::new(reqwest::Client::new(), &format!("{}/v1", server.uri()));
        let resp = backend
            .chat(
                "test-model",
                &[ChatMessage::user("hi")],
                Some("secret-key"),
            )
            .await
            .unwrap();
        assert_eq!(resp.content, "hello from mock");
        assert!(resp.backend_id.contains("test-model"));
    }

    #[tokio::test]
    async fn auth_error_maps_to_worker_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
            .mount(&server)
            .await;

        let backend =
            OpenAiCompatibleBackend::new(reqwest::Client::new(), &format!("{}/v1", server.uri()));
        let err = backend
            .chat("m", &[ChatMessage::user("hi")], Some("bad"))
            .await
            .unwrap_err();
        let msg = err.to_caller_message();
        assert!(msg.starts_with("worker unavailable:"), "{msg}");
        assert!(msg.contains("auth"), "{msg}");
    }
}
