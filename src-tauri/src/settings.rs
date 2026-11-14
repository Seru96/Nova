//! Ajustes de Nova que vive el backend (Rust).
//!
//! Se guardan en `settings.json` dentro del directorio de configuración de la app.
//! El backend es la fuente de verdad: así el Modo Confidencial no depende de que
//! el frontend lo restaure correctamente al arrancar.

use std::path::PathBuf;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersonalityPreset {
    /// Ingeniosa y con humor (por defecto).
    Humor,
    /// Seria, concisa y formal.
    Profesional,
    /// Cálida y cercana.
    Amable,
    /// Ironía afilada, sin pasarse.
    Sarcastica,
    /// Usa solo el texto de `personality_custom`.
    Personalizada,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Cómo quiere el usuario que Nova le llame (opcional).
    pub user_name: String,
    pub personality: PersonalityPreset,
    /// Instrucciones extra de personalidad escritas por el usuario.
    pub personality_custom: String,
    /// Modo Confidencial: bloquea en el backend cualquier petición a Claude.
    pub confidential: bool,
    /// Escalar automáticamente a Claude cuando el modelo local lo pida.
    pub auto_escalate: bool,
    /// Modelo de Claude para las escaladas.
    pub claude_model: String,
    /// Switch de escucha (micrófono + wake word).
    pub listening: bool,
    /// Leer en voz alta las respuestas a peticiones hechas por voz.
    pub speak_replies: bool,
    /// Nombre de la voz del sistema elegida (vacío = automática en español).
    pub voice_name: String,
    /// Permitir que Nova abra apps, escriba en ellas y pulse atajos.
    pub app_control: bool,
    /// Tema de la interfaz: "dark", "light" o "system".
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            user_name: String::new(),
            personality: PersonalityPreset::Humor,
            personality_custom: String::new(),
            confidential: false,
            auto_escalate: true,
            claude_model: "claude-opus-5-5".into(),
            listening: false,
            speak_replies: true,
            voice_name: String::new(),
            app_control: true,
            theme: "dark".into(),
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    inner: RwLock<Settings>,
}

impl SettingsStore {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("settings.json");
        let inner = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            inner: RwLock::new(inner),
        }
    }

    pub fn get(&self) -> Settings {
        self.inner.read().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Settings {
        let snapshot = {
            let mut s = self.inner.write();
            f(&mut s);
            s.clone()
        };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
        snapshot
    }

    pub fn is_confidential(&self) -> bool {
        self.inner.read().confidential
    }
}
