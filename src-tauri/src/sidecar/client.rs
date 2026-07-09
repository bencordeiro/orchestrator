//! HTTP client for CLIProxyAPI management + OpenAI surfaces.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One connected subscription account (from `/v0/management/auth-files`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthAccount {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub email: Option<String>,
    pub label: Option<String>,
    pub status: String,
    pub status_message: String,
    pub unavailable: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone)]
pub struct CliProxyClient {
    pub base_url: String,
    pub management_key: String,
    pub proxy_api_key: String,
    http: reqwest::Client,
}

impl CliProxyClient {
    pub fn new(base_url: impl Into<String>, management_key: String, proxy_api_key: String) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            management_key,
            proxy_api_key,
            http: reqwest::Client::new(),
        }
    }

    /// Health: root endpoint returns 200 when the process is up.
    pub async fn health(&self) -> Result<()> {
        let url = format!("{}/", self.base_url);
        let resp = self
            .http
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .context("sidecar health request failed")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("sidecar health status {}", resp.status()))
        }
    }

    pub async fn list_auth_files(&self) -> Result<Vec<AuthAccount>> {
        let url = format!("{}/v0/management/auth-files", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.management_key))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .context("list auth-files failed")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("list auth-files: {body}"));
        }
        let v: Value = resp.json().await?;
        let files = v
            .get("files")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for f in files {
            let name = f
                .get("name")
                .or_else(|| f.get("id"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            out.push(AuthAccount {
                id: name.clone(),
                name: name.clone(),
                provider: f
                    .get("provider")
                    .or_else(|| f.get("type"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                email: f
                    .get("email")
                    .or_else(|| f.get("account"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                label: f
                    .get("label")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                status: f
                    .get("status")
                    .and_then(|x| x.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                status_message: f
                    .get("status_message")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                unavailable: f
                    .get("unavailable")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
                disabled: f
                    .get("disabled")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
            });
        }
        Ok(out)
    }

    pub async fn delete_auth_file(&self, name: &str) -> Result<()> {
        let url = format!(
            "{}/v0/management/auth-files?name={}",
            self.base_url,
            urlencoding_encode(name)
        );
        let resp = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.management_key))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .context("delete auth-file failed")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(anyhow!("delete auth-file: {body}"))
        }
    }

    /// List models exposed on the OpenAI-compatible surface.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/v1/models", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.proxy_api_key))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .context("list models failed")?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("list models: {body}"));
        }
        let v: Value = resp.json().await?;
        let mut ids = Vec::new();
        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
            for m in arr {
                if let Some(id) = m.get("id").and_then(|x| x.as_str()) {
                    ids.push(id.to_string());
                }
            }
        }
        Ok(ids)
    }
}

fn urlencoding_encode(s: &str) -> String {
    // Minimal encoding for query values.
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '@' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

/// Map HTTP/backend failure text into a clear worker-unavailable reason fragment.
pub fn classify_proxy_error(status: u16, body: &str) -> String {
    let lower = body.to_lowercase();
    if status == 0 {
        return "subscription sidecar not running".into();
    }
    if status == 401 || status == 403 || lower.contains("invalid api key") || lower.contains("unauthorized") {
        return format!("oauth expired or revoked ({status}): {body}");
    }
    if status == 429
        || lower.contains("quota")
        || lower.contains("rate limit")
        || lower.contains("resource_exhausted")
    {
        return format!("provider quota exhausted ({status}): {body}");
    }
    format!("subscription backend error ({status}): {body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_quota_and_auth() {
        assert!(classify_proxy_error(429, "rate limit").contains("quota"));
        assert!(classify_proxy_error(401, "Invalid API key").contains("oauth expired"));
        assert!(classify_proxy_error(0, "").contains("not running"));
    }
}
