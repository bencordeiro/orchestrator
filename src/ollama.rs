//! Ollama discovery helpers (host probing only — profiles stay generic).

use serde::Deserialize;
use serde_json::Value;

use crate::error::{OrchestratorError, Result};

/// Default Ollama base (without `/v1`).
pub const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";

/// One installed model reported by Ollama.
#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq, Eq)]
pub struct OllamaModel {
    pub name: String,
    pub host: String,
    /// OpenAI-compatible base URL for this host (`{host}/v1`).
    pub openai_base_url: String,
}

/// Normalize a host string to an origin without trailing slash.
pub fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.is_empty() {
        return DEFAULT_OLLAMA_HOST.to_string();
    }
    if h.ends_with("/v1") {
        return h.trim_end_matches("/v1").trim_end_matches('/').to_string();
    }
    h.to_string()
}

/// Probe one Ollama host and list models via `GET /api/tags`.
pub async fn list_models_on_host(http: &reqwest::Client, host: &str) -> Result<Vec<OllamaModel>> {
    let host = normalize_host(host);
    let url = format!("{host}/api/tags");
    let resp = http
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| {
            OrchestratorError::Other(anyhow::anyhow!("ollama probe {host} failed: {e}"))
        })?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(OrchestratorError::Other(anyhow::anyhow!(
            "ollama {host}: {body}"
        )));
    }
    let v: Value = resp.json().await.map_err(|e| {
        OrchestratorError::Other(anyhow::anyhow!("ollama json: {e}"))
    })?;
    let mut out = Vec::new();
    if let Some(models) = v.get("models").and_then(|m| m.as_array()) {
        for m in models {
            let name = m
                .get("name")
                .or_else(|| m.get("model"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            out.push(OllamaModel {
                name: name.clone(),
                host: host.clone(),
                openai_base_url: format!("{host}/v1"),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Probe default + extra hosts; skip hosts that fail.
pub async fn discover_models(
    http: &reqwest::Client,
    extra_hosts: &[String],
) -> Vec<OllamaModel> {
    let mut hosts = vec![DEFAULT_OLLAMA_HOST.to_string()];
    for h in extra_hosts {
        let n = normalize_host(h);
        if !hosts.iter().any(|x| x == &n) {
            hosts.push(n);
        }
    }
    let mut all = Vec::new();
    for h in hosts {
        match list_models_on_host(http, &h).await {
            Ok(mut ms) => all.append(&mut ms),
            Err(e) => {
                tracing::debug!("ollama discover skip {h}: {e}");
            }
        }
    }
    all
}

/// Suggested profile id for an Ollama model on a host.
pub fn profile_id_for_model(host: &str, model: &str) -> String {
    let host_key = normalize_host(host)
        .replace("http://", "")
        .replace("https://", "")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    let model_key: String = model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("ollama-{host_key}-{model_key}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn normalize_strips_v1() {
        assert_eq!(
            normalize_host("http://localhost:11434/v1"),
            "http://localhost:11434"
        );
        assert_eq!(normalize_host(""), DEFAULT_OLLAMA_HOST);
    }

    #[tokio::test]
    async fn lists_models_from_stub() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [
                    { "name": "llama3.2:latest" },
                    { "name": "nomic-embed-text" }
                ]
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let models = list_models_on_host(&http, &server.uri()).await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "llama3.2:latest");
        assert!(models[0].openai_base_url.ends_with("/v1"));
    }

    #[tokio::test]
    async fn discover_skips_dead_hosts() {
        let http = reqwest::Client::new();
        let models = discover_models(&http, &["http://127.0.0.1:1".into()]).await;
        // Default localhost may or may not be up; dead host must not panic.
        let _ = models;
    }
}
