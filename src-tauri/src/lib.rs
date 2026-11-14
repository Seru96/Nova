//! Nova: asistente de escritorio local-first.

mod download;
mod intents;
mod llm;
mod persona;
mod router;
mod secrets;
mod settings;
mod voice;

use std::sync::Arc;

use tauri::{Emitter, Manager, State};
use tauri_plugin_sql::{Migration, MigrationKind};

use llm::claude::ClaudeClient;
use llm::local::{ChatMessage, EngineStatus, LocalEngine};
use router::Router;
use settings::{Settings, SettingsStore};
use voice::{tts::VoiceInfo, VoiceService};

struct AppState {
    settings: Arc<SettingsStore>,
    engine: Arc<LocalEngine>,
    router: Arc<Router>,
    voice: Arc<VoiceService>,
}

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---------- Ajustes ----------

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.get()
}

/// Guarda los ajustes editables desde la pantalla de Ajustes.
/// `confidential` y `listening` tienen comandos propios y aquí se ignoran.
#[tauri::command]
fn save_settings(app: tauri::AppHandle, state: State<AppState>, settings: Settings) -> Settings {
    let s = state.settings.update(|cur| {
        let confidential = cur.confidential;
        let listening = cur.listening;
        *cur = settings;
        cur.confidential = confidential;
        cur.listening = listening;
    });
    let _ = app.emit("settings://changed", &s);
    s
}

#[tauri::command]
fn set_confidential(app: tauri::AppHandle, state: State<AppState>, on: bool) -> Settings {
    let s = state.settings.update(|s| s.confidential = on);
    let _ = app.emit("settings://changed", &s);
    s
}

#[tauri::command]
fn set_listening(app: tauri::AppHandle, state: State<AppState>, on: bool) -> R<Settings> {
    if on {
        state.voice.start().map_err(|e| format!("{e:#}"))?;
    } else {
        state.voice.stop();
    }
    let s = state.settings.update(|s| s.listening = on);
    let _ = app.emit("settings://changed", &s);
    Ok(s)
}

// ---------- API key (llavero del sistema) ----------

#[tauri::command]
fn has_api_key() -> bool {
    secrets::has_api_key()
}

#[tauri::command]
fn set_api_key(key: String) -> R<()> {
    if !key.trim().starts_with("sk-ant-") {
        return Err("La clave debería empezar por sk-ant-".into());
    }
    secrets::set_api_key(&key)
}

#[tauri::command]
fn clear_api_key() -> R<()> {
    secrets::clear_api_key()
}

// ---------- Motor local ----------

#[tauri::command]
fn engine_status(state: State<AppState>) -> EngineStatus {
    state.engine.status()
}

#[tauri::command]
fn engine_setup(state: State<AppState>) {
    let engine = state.engine.clone();
    tauri::async_runtime::spawn(async move {
        let _ = engine.setup().await;
    });
}

// ---------- Chat ----------

#[tauri::command]
fn chat_send(
    state: State<AppState>,
    id: String,
    history: Vec<ChatMessage>,
    text: String,
    via_voice: bool,
) {
    state.router.send(id, history, text, via_voice);
}

#[tauri::command]
fn chat_cancel(state: State<AppState>) {
    state.router.cancel();
}

// ---------- Voz ----------

#[tauri::command]
fn voice_push_to_talk(state: State<AppState>) -> R<()> {
    state.voice.push_to_talk().map_err(err)
}

#[tauri::command]
fn speak(state: State<AppState>, text: String) {
    let voice = state.settings.get().voice_name;
    state.voice.speaker.speak(&text, &voice);
}

#[tauri::command]
fn stop_speaking(state: State<AppState>) {
    state.voice.speaker.stop();
}

#[tauri::command]
fn list_voices(state: State<AppState>) -> Vec<VoiceInfo> {
    state.voice.speaker.voices()
}

fn migrations() -> Vec<Migration> {
    vec![Migration {
        version: 1,
        description: "conversaciones y mensajes",
        sql: r#"
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT 'Conversación',
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                role TEXT NOT NULL CHECK (role IN ('user','assistant','system')),
                content TEXT NOT NULL,
                engine TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id, created_at);
        "#,
        kind: MigrationKind::Up,
    }]
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:nova.db", migrations())
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;

            let settings = Arc::new(SettingsStore::load(config_dir));
            let engine = LocalEngine::new(handle.clone(), data_dir.join("engine"));
            let claude = Arc::new(ClaudeClient::new(settings.clone()));
            let router = Router::new(handle.clone(), settings.clone(), engine.clone(), claude);
            let voice = VoiceService::new(handle.clone(), engine.clone());

            // Arranque del motor local en segundo plano (descarga si hace falta).
            {
                let engine = engine.clone();
                let voice = voice.clone();
                let settings = settings.clone();
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if engine.setup().await.is_ok() && settings.get().listening {
                        if let Err(e) = voice.start() {
                            log::warn!("no se pudo reactivar la escucha: {e:#}");
                            let s = settings.update(|s| s.listening = false);
                            let _ = handle.emit("settings://changed", s);
                        }
                    }
                });
            }

            app.manage(AppState {
                settings,
                engine,
                router,
                voice,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            set_confidential,
            set_listening,
            has_api_key,
            set_api_key,
            clear_api_key,
            engine_status,
            engine_setup,
            chat_send,
            chat_cancel,
            voice_push_to_talk,
            speak,
            stop_speaking,
            list_voices,
        ])
        .build(tauri::generate_context!())
        .expect("error al iniciar Nova")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.voice.stop();
                    state.engine.stop();
                }
            }
        });
}
