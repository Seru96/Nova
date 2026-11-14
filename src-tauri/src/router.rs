//! Enrutador cognitivo híbrido: local por defecto, Claude cuando hace falta.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::task::AbortHandle;

use crate::intents::{self, PrivacyCommand};
use crate::llm::claude::{ClaudeClient, ConfidentialBlocked, MissingApiKey};
use crate::llm::local::{ChatMessage, LocalEngine, LocalOutcome};
use crate::persona;
use crate::settings::SettingsStore;

/// Cuántos mensajes del historial se envían al modelo (la memoria compacta llega en fase 2).
const HISTORY_WINDOW: usize = 20;

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
    current: Mutex<Option<(String, AbortHandle)>>,
}

impl Router {
    pub fn new(
        app: AppHandle,
        settings: Arc<SettingsStore>,
        local: Arc<LocalEngine>,
        claude: Arc<ClaudeClient>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app,
            settings,
            local,
            claude,
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
        let system = persona::system_prompt(&settings, true, via_voice);
        self.emit_route(id, Engine::Local, None);
        let result = self
            .local
            .chat(&system, history, can_escalate, self.delta_sink(id))
            .await;
        match result {
            Ok(LocalOutcome::Text(t)) => self.emit_done(id, Engine::Local, t, None, via_voice),
            Ok(LocalOutcome::Escalate { task, reason }) => {
                let note = if reason.is_empty() {
                    "Esto es para Claude".to_string()
                } else {
                    format!("Escalando a Claude: {reason}")
                };
                let _ = task; // El historial completo ya contiene la petición.
                self.run_claude(id, history, Some(note), via_voice).await;
            }
            Err(e) => self.emit_done(id, Engine::Local, String::new(), Some(format!("{e:#}")), via_voice),
        }
    }

    async fn run_claude(&self, id: &str, history: &[ChatMessage], note: Option<String>, via_voice: bool) {
        let settings = self.settings.get();
        let system = persona::system_prompt(&settings, false, via_voice);
        self.emit_route(id, Engine::Claude, note);
        match self.claude.chat(&system, history, self.delta_sink(id)).await {
            Ok(t) => self.emit_done(id, Engine::Claude, t, None, via_voice),
            Err(e) if e.downcast_ref::<ConfidentialBlocked>().is_some() => {
                self.emit_route(id, Engine::Local, Some("Modo Confidencial: resuelvo en local".into()));
                Box::pin(self.run_local(id, history, false, via_voice)).await;
            }
            Err(e) if e.downcast_ref::<MissingApiKey>().is_some() => {
                self.emit_route(
                    id,
                    Engine::Local,
                    Some("Sin API key de Claude: lo intento en local (añádela en Ajustes)".into()),
                );
                Box::pin(self.run_local(id, history, false, via_voice)).await;
            }
            Err(e) => self.emit_done(id, Engine::Claude, String::new(), Some(format!("{e:#}")), via_voice),
        }
    }
}
