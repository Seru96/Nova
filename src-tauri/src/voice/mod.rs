//! Servicio de voz: micrófono → VAD → whisper → wake word "Oye Nova" → orden.

pub mod audio;
pub mod stt;
pub mod tts;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::intents::{detect_wake, Wake};
use crate::llm::local::LocalEngine;

/// Tras un "Oye Nova" a secas, cuánto tiempo esperamos la orden.
const AWAKE_WINDOW: Duration = Duration::from_secs(8);

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoicePhase {
    Off,
    Loading,
    Idle,
    Awake,
    Transcribing,
}

#[derive(Clone, Serialize)]
struct VoiceState {
    phase: VoicePhase,
    error: Option<String>,
}

pub struct VoiceService {
    app: AppHandle,
    engine: Arc<LocalEngine>,
    running: Mutex<Option<Arc<AtomicBool>>>,
    awake_until: Arc<Mutex<Option<Instant>>>,
    stt: Arc<Mutex<Option<Arc<stt::Stt>>>>,
    pub speaker: tts::Speaker,
    speaking: Arc<AtomicBool>,
}

impl VoiceService {
    pub fn new(app: AppHandle, engine: Arc<LocalEngine>) -> Arc<Self> {
        let speaking = Arc::new(AtomicBool::new(false));
        Arc::new(Self {
            app,
            engine,
            running: Mutex::new(None),
            awake_until: Arc::new(Mutex::new(None)),
            stt: Arc::new(Mutex::new(None)),
            speaker: tts::Speaker::new(speaking.clone()),
            speaking,
        })
    }

    fn emit(app: &AppHandle, phase: VoicePhase, error: Option<String>) {
        let _ = app.emit("voice://state", VoiceState { phase, error });
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().is_some()
    }

    /// Activa la escucha. Devuelve error si no hay micrófono o modelo.
    pub fn start(&self) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }
        let model = self.engine.stt_model_path();
        if !model.exists() {
            bail!("El modelo de voz aún no se ha descargado");
        }

        let running = Arc::new(AtomicBool::new(true));
        let (utt_tx, utt_rx) = std::sync::mpsc::channel::<Vec<f32>>();
        let ready = audio::spawn_capture(running.clone(), self.speaking.clone(), utt_tx);
        match ready.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                running.store(false, Ordering::Relaxed);
                bail!("No se pudo abrir el micrófono: {e}");
            }
            Err(_) => {
                running.store(false, Ordering::Relaxed);
                bail!("El micrófono no respondió (¿permiso denegado?)");
            }
        }
        *self.running.lock() = Some(running.clone());

        let app = self.app.clone();
        let stt_slot = self.stt.clone();
        let awake = self.awake_until.clone();
        std::thread::Builder::new()
            .name("nova-stt".into())
            .spawn(move || {
                // Carga perezosa del modelo (se reutiliza entre activaciones).
                let stt = {
                    let existing = stt_slot.lock().clone();
                    match existing {
                        Some(s) => s,
                        None => {
                            Self::emit(&app, VoicePhase::Loading, None);
                            match stt::Stt::load(&model) {
                                Ok(s) => {
                                    let s = Arc::new(s);
                                    *stt_slot.lock() = Some(s.clone());
                                    s
                                }
                                Err(e) => {
                                    running.store(false, Ordering::Relaxed);
                                    Self::emit(&app, VoicePhase::Off, Some(format!("{e:#}")));
                                    return;
                                }
                            }
                        }
                    }
                };
                Self::emit(&app, VoicePhase::Idle, None);

                while running.load(Ordering::Relaxed) {
                    // Caducidad de la ventana de "despierta".
                    {
                        let mut a = awake.lock();
                        if a.is_some_and(|t| Instant::now() > t) {
                            *a = None;
                            Self::emit(&app, VoicePhase::Idle, None);
                        }
                    }
                    let Ok(utt) = utt_rx.recv_timeout(Duration::from_millis(250)) else {
                        continue;
                    };
                    let was_awake = awake.lock().is_some();
                    Self::emit(&app, VoicePhase::Transcribing, None);
                    let text = match stt.transcribe(&utt) {
                        Ok(t) => t,
                        Err(e) => {
                            log::warn!("transcripción: {e:#}");
                            String::new()
                        }
                    };
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                    if !text.is_empty() {
                        let _ = app.emit("voice://transcript", &text);
                    }

                    let command = if was_awake && !text.is_empty() {
                        // Estábamos esperando la orden: todo lo dicho cuenta (quitando un "Nova" inicial).
                        match detect_wake(&text) {
                            Wake::WithCommand(c) => Some(c),
                            Wake::Bare => None,
                            Wake::None => Some(text.clone()),
                        }
                    } else {
                        match detect_wake(&text) {
                            Wake::WithCommand(c) => Some(c),
                            Wake::Bare => {
                                *awake.lock() = Some(Instant::now() + AWAKE_WINDOW);
                                let _ = app.emit("voice://wake", ());
                                Self::emit(&app, VoicePhase::Awake, None);
                                continue;
                            }
                            Wake::None => None,
                        }
                    };

                    if let Some(cmd) = command {
                        *awake.lock() = None;
                        let _ = app.emit("voice://command", cmd);
                    }
                    let phase = if awake.lock().is_some() {
                        VoicePhase::Awake
                    } else {
                        VoicePhase::Idle
                    };
                    Self::emit(&app, phase, None);
                }
                Self::emit(&app, VoicePhase::Off, None);
            })?;
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(r) = self.running.lock().take() {
            r.store(false, Ordering::Relaxed);
        }
        *self.awake_until.lock() = None;
        Self::emit(&self.app, VoicePhase::Off, None);
    }

    /// Botón "hablar": la siguiente frase se toma como orden sin decir "Oye Nova".
    pub fn push_to_talk(&self) -> Result<()> {
        if !self.is_running() {
            bail!("Activa la escucha primero");
        }
        self.speaker.stop();
        *self.awake_until.lock() = Some(Instant::now() + AWAKE_WINDOW);
        Self::emit(&self.app, VoicePhase::Awake, None);
        Ok(())
    }
}
