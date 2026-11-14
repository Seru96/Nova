//! Síntesis de voz offline con las voces del sistema.
//!
//! - macOS: comando `say` con una voz femenina en español (Mónica, Paulina...).
//! - Windows/Linux: crate `tts` (WinRT / speech-dispatcher).
//!
//! Gratis, sin conexión y sin descargar nada. Mientras habla se silencia el
//! micrófono para que Nova no se escuche a sí misma.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct VoiceInfo {
    pub name: String,
    pub language: String,
}

enum Cmd {
    Speak(String, String),
    Stop,
    List(Sender<Vec<VoiceInfo>>),
}

#[derive(Clone)]
pub struct Speaker {
    tx: Sender<Cmd>,
}

impl Speaker {
    /// `speaking` se pone a true mientras se reproduce audio.
    pub fn new(speaking: Arc<AtomicBool>) -> Self {
        let (tx, rx) = channel::<Cmd>();
        std::thread::Builder::new()
            .name("nova-tts".into())
            .spawn(move || backend::run(rx, speaking))
            .expect("hilo TTS");
        Self { tx }
    }

    pub fn speak(&self, text: &str, voice: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let _ = self.tx.send(Cmd::Speak(text.to_string(), voice.to_string()));
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }

    pub fn voices(&self) -> Vec<VoiceInfo> {
        let (tx, rx) = channel();
        let _ = self.tx.send(Cmd::List(tx));
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap_or_default()
    }
}

/// Voces femeninas en español conocidas, por orden de preferencia.
const PREFERRED: &[&str] = &[
    "Mónica", "Monica", "Paulina", "Marisol", "Angélica", "Helena", "Laura", "Sabina", "Elvira",
    "Dalia", "Paloma",
];

fn pick_default(voices: &[VoiceInfo]) -> Option<String> {
    let spanish: Vec<&VoiceInfo> = voices
        .iter()
        .filter(|v| v.language.to_lowercase().starts_with("es"))
        .collect();
    for p in PREFERRED {
        if let Some(v) = spanish.iter().find(|v| v.name.contains(p)) {
            return Some(v.name.clone());
        }
    }
    spanish.first().map(|v| v.name.clone())
}

#[cfg(target_os = "macos")]
mod backend {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::Receiver;

    fn list() -> Vec<VoiceInfo> {
        let Ok(out) = Command::new("say").args(["-v", "?"]).output() else {
            return vec![];
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                // "Mónica              es_ES    # Hola, me llamo Mónica."
                let head = l.split('#').next()?.trim_end();
                let (name, lang) = head.rsplit_once(char::is_whitespace)?;
                Some(VoiceInfo {
                    name: name.trim().to_string(),
                    language: lang.trim().replace('_', "-"),
                })
            })
            .collect()
    }

    pub fn run(rx: Receiver<Cmd>, speaking: Arc<AtomicBool>) {
        let voices = list();
        let default_voice = pick_default(&voices);
        let mut child: Option<Child> = None;
        loop {
            // Comprobamos si el `say` actual terminó.
            let cmd = rx.recv_timeout(std::time::Duration::from_millis(150));
            if let Some(c) = child.as_mut() {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    child = None;
                    speaking.store(false, Ordering::Relaxed);
                }
            }
            match cmd {
                Ok(Cmd::Speak(text, voice)) => {
                    if let Some(mut c) = child.take() {
                        let _ = c.kill();
                    }
                    let voice = if voice.is_empty() { default_voice.clone() } else { Some(voice) };
                    let mut command = Command::new("say");
                    if let Some(v) = &voice {
                        command.args(["-v", v]);
                    }
                    command.args(["-r", "190"]).arg(text).stdout(Stdio::null()).stderr(Stdio::null());
                    if let Ok(c) = command.spawn() {
                        speaking.store(true, Ordering::Relaxed);
                        child = Some(c);
                    }
                }
                Ok(Cmd::Stop) => {
                    if let Some(mut c) = child.take() {
                        let _ = c.kill();
                    }
                    speaking.store(false, Ordering::Relaxed);
                }
                Ok(Cmd::List(tx)) => {
                    let _ = tx.send(voices.clone());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod backend {
    use super::*;
    use std::sync::mpsc::Receiver;

    pub fn run(rx: Receiver<Cmd>, speaking: Arc<AtomicBool>) {
        let mut tts = match tts::Tts::default() {
            Ok(t) => Some(t),
            Err(e) => {
                log::error!("TTS no disponible: {e}");
                None
            }
        };
        let voices: Vec<(tts::Voice, VoiceInfo)> = tts
            .as_ref()
            .and_then(|t| t.voices().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|v| {
                let info = VoiceInfo {
                    name: v.name(),
                    language: v.language().to_string(),
                };
                (v, info)
            })
            .collect();
        let infos: Vec<VoiceInfo> = voices.iter().map(|(_, i)| i.clone()).collect();
        let default_voice = pick_default(&infos);
        let mut current_voice = String::new();

        loop {
            let cmd = rx.recv_timeout(std::time::Duration::from_millis(150));
            if let Some(t) = tts.as_ref() {
                if speaking.load(Ordering::Relaxed) && !t.is_speaking().unwrap_or(false) {
                    speaking.store(false, Ordering::Relaxed);
                }
            }
            match cmd {
                Ok(Cmd::Speak(text, voice)) => {
                    let Some(t) = tts.as_mut() else { continue };
                    let wanted = if voice.is_empty() {
                        default_voice.clone().unwrap_or_default()
                    } else {
                        voice
                    };
                    if wanted != current_voice {
                        if let Some((v, _)) = voices.iter().find(|(_, i)| i.name == wanted) {
                            let _ = t.set_voice(v);
                        }
                        current_voice = wanted;
                    }
                    if t.speak(text, true).is_ok() {
                        speaking.store(true, Ordering::Relaxed);
                    }
                }
                Ok(Cmd::Stop) => {
                    if let Some(t) = tts.as_mut() {
                        let _ = t.stop();
                    }
                    speaking.store(false, Ordering::Relaxed);
                }
                Ok(Cmd::List(tx)) => {
                    let _ = tx.send(infos.clone());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
    }
}
