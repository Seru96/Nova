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

    /// Una vuelta de conversación con herramientas. `messages` va en formato de la API
    /// (empieza por `user`); `tools` en formato OpenAI (se convierten aquí).
    pub async fn turn(
        &self,
        system: &str,
        messages: &[Value],
        tools: &[Value],
        mut on_delta: impl FnMut(&str),
    ) -> Result<ClaudeTurn> {
        self.guard()?;
        let key = secrets::get_api_key().ok_or(MissingApiKey)?;
        let model = self.settings.get().claude_model;

        let mut body = json!({
            "model": model,
            "max_tokens": 16000,
            "stream": true,
            "system": system,
            "messages": messages,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.iter().map(to_anthropic_tool).collect());
        }
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

        let mut out = ClaudeTurn::default();
        // Bloques de contenido por índice: texto o tool_use (con su JSON a trozos).
        let mut blocks: Vec<(Value, String)> = Vec::new();
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
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                match v["type"].as_str() {
                    Some("content_block_start") => {
                        while blocks.len() <= idx {
                            blocks.push((Value::Null, String::new()));
                        }
                        blocks[idx].0 = v["content_block"].clone();
                    }
                    Some("content_block_delta") => {
                        if v["delta"]["type"] == "text_delta" {
                            if let Some(t) = v["delta"]["text"].as_str() {
                                out.text.push_str(t);
                                on_delta(t);
                                if let Some(b) = blocks.get_mut(idx) {
                                    b.1.push_str(t);
                                }
                            }
                        } else if v["delta"]["type"] == "input_json_delta" {
                            if let (Some(b), Some(t)) = (blocks.get_mut(idx), v["delta"]["partial_json"].as_str()) {
                                b.1.push_str(t);
                            }
                        }
                    }
                    Some("message_delta") => {
                        if v["delta"]["stop_reason"] == "refusal" {
                            let note = "\n\n(Claude ha declinado esta petición.)";
                            out.text.push_str(note);
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

        let mut content = Vec::new();
        for (block, acc) in blocks {
            match block["type"].as_str() {
                Some("text") => {
                    if !acc.is_empty() {
                        content.push(json!({"type": "text", "text": acc}));
                    }
                }
                Some("tool_use") => {
                    let input: Value = if acc.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&acc).unwrap_or(json!({}))
                    };
                    let id = block["id"].as_str().unwrap_or_default().to_string();
                    let name = block["name"].as_str().unwrap_or_default().to_string();
                    content.push(json!({"type": "tool_use", "id": id, "name": name, "input": input}));
                    out.tool_uses.push(ToolUse { id, name, input });
                }
                _ => {}
            }
        }
        out.content = Value::Array(content);
        Ok(out)
    }
}

#[derive(Debug, Default)]
pub struct ClaudeTurn {
    pub text: String,
    pub tool_uses: Vec<ToolUse>,
    /// Bloques de la respuesta, para devolverlos tal cual en la siguiente vuelta.
    pub content: Value,
}

#[derive(Debug)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Herramienta en formato OpenAI (la del modelo local) → formato de la API de Claude.
fn to_anthropic_tool(t: &Value) -> Value {
    let f = &t["function"];
    json!({
        "name": f["name"],
        "description": f["description"],
        "input_schema": f["parameters"],
    })
}

/// Los modelos que aceptan `fallbacks: "default"` (rescate ante rechazos de los clasificadores).
fn supports_default_fallback(model: &str) -> bool {
    model.starts_with("claude-opus-5")
        || model.starts_with("claude-sonnet-5-5")
        || model.starts_with("claude-fable-5")
}

/// La API exige empezar por `user` y alternar roles: fusionamos y limpiamos.
pub fn sanitize(history: &[ChatMessage]) -> Vec<Value> {
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
    fn converts_tools() {
        let t = json!({"type": "function", "function": {"name": "read_file", "description": "d", "parameters": {"type": "object"}}});
        let a = to_anthropic_tool(&t);
        assert_eq!(a["name"], "read_file");
        assert_eq!(a["input_schema"]["type"], "object");
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
        let err = client.turn("sys", &sanitize(&[m("user", "hola")]), &[], |_| {}).await.unwrap_err();
        assert!(err.downcast_ref::<ConfidentialBlocked>().is_some());
    }
}
