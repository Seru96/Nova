//! Enrutador cognitivo híbrido: local por defecto, Claude cuando hace falta.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::task::AbortHandle;

use crate::intents::{self, PrivacyCommand};
use crate::llm::claude::{sanitize, ClaudeClient, ConfidentialBlocked, MissingApiKey};
use crate::llm::local::{escalate_tool, ChatMessage, LocalEngine, LocalOutcome};
use crate::persona;
use crate::settings::SettingsStore;
use crate::skills::{self, Skills};
use crate::tools;

/// Cuántos mensajes del historial se envían al modelo (la memoria compacta llega en fase 2).
const HISTORY_WINDOW: usize = 20;
/// Máximo de rondas de herramientas por petición.
const MAX_TOOL_STEPS: usize = 12;

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Local,
    Claude,
    System,
}

#[derive(Clone, Serialize)]
struct DeltaEvent<'a> {
    id: &'a str,
    text: &'a str,
}

#[derive(Clone, Serialize)]
struct RouteEvent<'a> {
    id: &'a str,
    engine: Engine,
    note: Option<String>,
}

#[derive(Clone, Serialize)]
struct ActionEvent<'a> {
    id: &'a str,
    text: String,
}

#[derive(Clone, Serialize)]
struct DoneEvent<'a> {
    id: &'a str,
    engine: Engine,
    text: String,
    error: Option<String>,
    via_voice: bool,
}

pub struct Router {
    app: AppHandle,
    settings: Arc<SettingsStore>,
    local: Arc<LocalEngine>,
    claude: Arc<ClaudeClient>,
    skills: Arc<Skills>,
    current: Mutex<Option<(String, AbortHandle)>>,
}

impl Router {
    pub fn new(
        app: AppHandle,
        settings: Arc<SettingsStore>,
        local: Arc<LocalEngine>,
        claude: Arc<ClaudeClient>,
        skills: Arc<Skills>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app,
            settings,
            local,
            claude,
            skills,
            current: Mutex::new(None),
        })
    }

    pub fn cancel(&self) {
        if let Some((_, h)) = self.current.lock().take() {
            h.abort();
        }
    }

    /// Lanza la respuesta en segundo plano y devuelve enseguida.
    pub fn send(self: &Arc<Self>, id: String, history: Vec<ChatMessage>, text: String, via_voice: bool) {
        self.cancel();
        let me = self.clone();
        let id2 = id.clone();
        let handle = tauri::async_runtime::spawn(async move {
            me.handle(&id2, history, text, via_voice).await;
        });
        *self.current.lock() = Some((id, handle.inner().abort_handle()));
    }

    fn emit_route(&self, id: &str, engine: Engine, note: Option<String>) {
        let _ = self.app.emit("chat://route", RouteEvent { id, engine, note });
    }

    fn emit_done(&self, id: &str, engine: Engine, text: String, error: Option<String>, via_voice: bool) {
        let _ = self.app.emit(
            "chat://done",
            DoneEvent {
                id,
                engine,
                text,
                error,
                via_voice,
            },
        );
    }

    fn emit_action(&self, id: &str, text: String) {
        let _ = self.app.emit("chat://action", ActionEvent { id, text });
    }

    fn delta_sink<'a>(&'a self, id: &'a str) -> impl FnMut(&str) + 'a {
        move |t: &str| {
            let _ = self.app.emit("chat://delta", DeltaEvent { id, text: t });
        }
    }

    fn set_confidential(&self, on: bool) {
        let s = self.settings.update(|s| s.confidential = on);
        let _ = self.app.emit("settings://changed", s);
    }

    async fn handle(&self, id: &str, mut history: Vec<ChatMessage>, text: String, via_voice: bool) {
        // 1) Órdenes de privacidad por texto/voz.
        if let Some(cmd) = intents::privacy_command(&text) {
            let on = cmd == PrivacyCommand::Enable;
            self.set_confidential(on);
            if intents::is_bare_command(&text) {
                let reply = if on {
                    "Modo Confidencial activado. Nada sale de este ordenador hasta que me digas lo contrario."
                } else {
                    "Modo Confidencial desactivado. Vuelvo a poder consultar a Claude si hace falta."
                };
                self.emit_done(id, Engine::System, reply.into(), None, via_voice);
                return;
            }
        }

        if history.len() > HISTORY_WINDOW {
            history.drain(..history.len() - HISTORY_WINDOW);
        }
        history.push(ChatMessage {
            role: "user".into(),
            content: text.clone(),
        });

        let settings = self.settings.get();

        // 2) Petición explícita a Claude.
        if intents::wants_claude(&text) {
            if settings.confidential {
                self.emit_route(id, Engine::Local, Some("Modo Confidencial: no consulto a Claude".into()));
                self.run_local(id, &history, false, via_voice).await;
            } else {
                self.run_claude(id, &history, None, via_voice).await;
            }
            return;
        }

        // 3) Local por defecto, con la herramienta de escalado si está permitido.
        let can_escalate = settings.auto_escalate && !settings.confidential;
        self.run_local(id, &history, can_escalate, via_voice).await;
    }

    async fn run_local(&self, id: &str, history: &[ChatMessage], can_escalate: bool, via_voice: bool) {
        if !self.local.is_ready() {
            self.emit_done(
                id,
                Engine::Local,
                String::new(),
                Some("El modelo local todavía no está listo.".into()),
                via_voice,
            );
            return;
        }
        let settings = self.settings.get();
        let system = persona::system_prompt(&settings, true, via_voice) + &self.skills_prompt(history);
        self.emit_route(id, Engine::Local, None);

        let mut tools = Vec::new();
        if can_escalate {
            tools.push(escalate_tool());
        }
        tools.extend(tools::definitions(&settings));

        let mut messages = vec![json!({"role": "system", "content": system})];
        messages.extend(history.iter().map(|m| json!({"role": m.role, "content": m.content})));

        let mut said = String::new();
        for _ in 0..MAX_TOOL_STEPS {
            let result = self.local.chat(&messages, &tools, self.delta_sink(id)).await;
            match result {
                Ok(LocalOutcome::Text(t)) => {
                    said.push_str(&t);
                    self.emit_done(id, Engine::Local, said, None, via_voice);
                    return;
                }
                Ok(LocalOutcome::ToolCalls { content, calls }) => {
                    if let Some(esc) = calls.iter().find(|c| c.name == "escalate_to_claude") {
                        let args: Value = serde_json::from_str(&esc.arguments).unwrap_or(Value::Null);
                        let note = match args["reason"].as_str() {
                            Some(r) if !r.is_empty() => format!("Escalando a Claude: {r}"),
                            _ => "Esto es para Claude".to_string(),
                        };
                        self.run_claude(id, history, Some(note), via_voice).await;
                        return;
                    }
                    if !content.trim().is_empty() {
                        said.push_str(&content);
                        said.push_str("\n\n");
                    }
                    messages.push(json!({
                        "role": "assistant",
                        "content": content,
                        "tool_calls": calls.iter().map(|c| json!({
                            "id": c.id,
                            "type": "function",
                            "function": {"name": c.name, "arguments": c.arguments},
                        })).collect::<Vec<_>>(),
                    }));
                    for c in &calls {
                        let args: Value = serde_json::from_str(&c.arguments).unwrap_or(Value::Null);
                        if tools::visible(&c.name) {
                            self.emit_action(id, tools::describe(&c.name, &args));
                        }
                        let output = tools::execute(&self.app, &self.skills, &settings, &c.name, &args).await;
                        messages.push(json!({"role": "tool", "tool_call_id": c.id, "content": output}));
                    }
                }
                Err(e) => {
                    self.emit_done(id, Engine::Local, String::new(), Some(format!("{e:#}")), via_voice);
                    return;
                }
            }
        }
        if said.trim().is_empty() {
            said = "Hecho.".into();
        }
        self.emit_done(id, Engine::Local, said, None, via_voice);
    }

    /// Skills que encajan con la última petición + la lista de todas.
    fn skills_prompt(&self, history: &[ChatMessage]) -> String {
        let last = history.iter().rev().find(|m| m.role == "user").map(|m| m.content.as_str()).unwrap_or("");
        let all = self.skills.all();
        let matched = skills::matching(&all, last);
        skills::prompt_section(&all, &matched)
    }

    async fn run_claude(&self, id: &str, history: &[ChatMessage], note: Option<String>, via_voice: bool) {
        let settings = self.settings.get();
        let system = persona::system_prompt(&settings, false, via_voice) + &self.skills_prompt(history);
        self.emit_route(id, Engine::Claude, note);
        let tool_defs = tools::definitions(&settings);
        let mut messages = sanitize(history);
        let mut said = String::new();

        for _ in 0..MAX_TOOL_STEPS * 2 {
            let turn = match self.claude.turn(&system, &messages, &tool_defs, self.delta_sink(id)).await {
                Ok(t) => t,
                Err(e) if e.downcast_ref::<ConfidentialBlocked>().is_some() => {
                    self.emit_route(id, Engine::Local, Some("Modo Confidencial: resuelvo en local".into()));
                    Box::pin(self.run_local(id, history, false, via_voice)).await;
                    return;
                }
                Err(e) if e.downcast_ref::<MissingApiKey>().is_some() => {
                    self.emit_route(
                        id,
                        Engine::Local,
                        Some("Sin API key de Claude: lo intento en local (añádela en Ajustes)".into()),
                    );
                    Box::pin(self.run_local(id, history, false, via_voice)).await;
                    return;
                }
                Err(e) => {
                    self.emit_done(id, Engine::Claude, said, Some(format!("{e:#}")), via_voice);
                    return;
                }
            };
            if !turn.text.trim().is_empty() {
                if !said.is_empty() {
                    said.push_str("\n\n");
                }
                said.push_str(&turn.text);
            }
            if turn.tool_uses.is_empty() {
                self.emit_done(id, Engine::Claude, said, None, via_voice);
                return;
            }
            messages.push(json!({"role": "assistant", "content": turn.content}));
            let mut results = Vec::new();
            for t in &turn.tool_uses {
                if tools::visible(&t.name) {
                    self.emit_action(id, tools::describe(&t.name, &t.input));
                }
                let output = tools::execute(&self.app, &self.skills, &settings, &t.name, &t.input).await;
                let is_error = output.starts_with("Error:");
                results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": t.id,
                    "content": output,
                    "is_error": is_error,
                }));
            }
            messages.push(json!({"role": "user", "content": results}));
        }
        if said.trim().is_empty() {
            said = "Hecho.".into();
        }
        self.emit_done(id, Engine::Claude, said, None, via_voice);
    }
}
