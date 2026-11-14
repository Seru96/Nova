//! Cliente de la API de Claude (Anthropic) con streaming.
//!
//! El Modo Confidencial se comprueba AQUÍ, justo antes de abrir la conexión,
//! no en la UI: ningún camino del código puede enviar datos a Claude mientras
//! el modo está activo.

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::download::http_client;
use crate::llm::local::ChatMessage;
use crate::secrets;
use crate::settings::SettingsStore;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";

#[derive(Debug)]
pub struct ConfidentialBlocked;

impl std::fmt::Display for ConfidentialBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Modo Confidencial activo: conexión con Claude bloqueada")
    }
}
impl std::error::Error for ConfidentialBlocked {}

#[derive(Debug)]
pub struct MissingApiKey;

impl std::fmt::Display for MissingApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "No hay API key de Claude configurada")
    }
}
impl std::error::Error for MissingApiKey {}

pub struct ClaudeClient {
    settings: Arc<SettingsStore>,
}

impl ClaudeClient {
    pub fn new(settings: Arc<SettingsStore>) -> Self {
        Self { settings }
    }

    fn guard(&self) -> Result<()> {
        if self.settings.is_confidential() {
            return Err(ConfidentialBlocked.into());
        }
        Ok(())
    }

    pub async fn chat(
        &self,
        system: &str,
        history: &[ChatMessage],
        mut on_delta: impl FnMut(&str),
    ) -> Result<String> {
        self.guard()?;
        let key = secrets::get_api_key().ok_or(MissingApiKey)?;
        let model = self.settings.get().claude_model;

        let mut body = json!({
            "model": model,
            "max_tokens": 16000,
            "stream": true,
            "system": system,
            "messages": sanitize(history),
        });
        let with_fallback = supports_default_fallback(&model);
        if with_fallback {
            body["fallbacks"] = json!("default");
        }

        let mut req = http_client()
            .post(API_URL)
            .header("x-api-key", key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json");
        if with_fallback {
            req = req.header("anthropic-beta", "server-side-fallback-2026-07-01");
        }

        // Segunda comprobación inmediatamente antes de salir a la red.
        self.guard()?;
        let resp = req.json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or(text);
            bail!("Claude respondió {status}: {msg}");
        }

        let mut out = String::new();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            // Si el usuario activa el Modo Confidencial a mitad de respuesta, cortamos.
            self.guard()?;
            buf.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(nl) = buf.find('\n') {
                let line: String = buf.drain(..=nl).collect();
                let Some(data) = line.trim().strip_prefix("data:") else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(data.trim()) else {
                    continue;
                };
                match v["type"].as_str() {
                    Some("content_block_delta") => {
                        if v["delta"]["type"] == "text_delta" {
                            if let Some(t) = v["delta"]["text"].as_str() {
                                out.push_str(t);
                                on_delta(t);
                            }
                        }
                    }
                    Some("message_delta") => {
                        if v["delta"]["stop_reason"] == "refusal" {
                            let note = "\n\n(Claude ha declinado esta petición.)";
                            out.push_str(note);
                            on_delta(note);
                        }
                    }
                    Some("error") => {
                        let msg = v["error"]["message"].as_str().unwrap_or("error desconocido");
                        return Err(anyhow!("Claude: {msg}"));
                    }
                    _ => {}
                }
            }
        }
        Ok(out)
    }
}

/// Los modelos que aceptan `fallbacks: "default"` (rescate ante rechazos de los clasificadores).
fn supports_default_fallback(model: &str) -> bool {
    model.starts_with("claude-opus-5")
        || model.starts_with("claude-sonnet-5-5")
        || model.starts_with("claude-fable-5")
}

/// La API exige empezar por `user` y alternar roles: fusionamos y limpiamos.
fn sanitize(history: &[ChatMessage]) -> Vec<Value> {
    let mut out: Vec<(String, String)> = Vec::new();
    for m in history {
        let role = if m.role == "assistant" { "assistant" } else { "user" };
        if m.content.trim().is_empty() {
            continue;
        }
        if out.is_empty() && role == "assistant" {
            continue;
        }
        match out.last_mut() {
            Some((r, c)) if r == role => {
                c.push_str("\n\n");
                c.push_str(&m.content);
            }
            _ => out.push((role.to_string(), m.content.clone())),
        }
    }
    out.into_iter()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(role: &str, c: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: c.into(),
        }
    }

    #[test]
    fn sanitize_alternates_and_starts_with_user() {
        let h = vec![
            m("assistant", "hola"),
            m("user", "a"),
            m("user", "b"),
            m("assistant", "c"),
        ];
        let s = sanitize(&h);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0]["role"], "user");
        assert_eq!(s[0]["content"], "a\n\nb");
        assert_eq!(s[1]["role"], "assistant");
    }

    #[tokio::test]
    async fn confidential_mode_blocks_before_network() {
        let dir = std::env::temp_dir().join(format!("nova-test-{}", std::process::id()));
        let store = Arc::new(SettingsStore::load(dir));
        store.update(|s| s.confidential = true);
        let client = ClaudeClient::new(store);
        let err = client.chat("sys", &[m("user", "hola")], |_| {}).await.unwrap_err();
        assert!(err.downcast_ref::<ConfidentialBlocked>().is_some());
    }
}
