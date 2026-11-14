//! Reconocimiento de voz offline con whisper.cpp (vía whisper-rs).

use std::path::Path;

use anyhow::{Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Stt {
    ctx: WhisperContext,
}

impl Stt {
    pub fn load(model: &Path) -> Result<Self> {
        let path = model.to_str().context("ruta de modelo no válida")?;
        let ctx = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .context("no se pudo cargar el modelo de voz")?;
        Ok(Self { ctx })
    }

    /// Transcribe audio mono 16 kHz en español.
    pub fn transcribe(&self, audio: &[f32]) -> Result<String> {
        let mut state = self.ctx.create_state()?;
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some("es"));
        p.set_translate(false);
        p.set_no_context(true);
        p.set_single_segment(true);
        p.set_suppress_blank(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        // Sesgo hacia el nombre de la asistente para que "Nova" se reconozca bien.
        p.set_initial_prompt("Oye Nova.");
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(6))
            .unwrap_or(4) as i32;
        p.set_n_threads(threads);

        // whisper necesita al menos ~1 s de audio; rellenamos con silencio.
        let mut buf;
        let audio = if audio.len() < 16_000 + 1_600 {
            buf = audio.to_vec();
            buf.resize(16_000 + 1_600, 0.0);
            &buf[..]
        } else {
            audio
        };

        state.full(p, audio)?;
        let mut text = String::new();
        for seg in state.as_iter() {
            if let Ok(t) = seg.to_str_lossy() {
                text.push_str(&t);
            }
        }
        Ok(clean(&text))
    }
}

/// Quita marcas típicas de whisper en silencio/ruido ("[Música]", "(risas)"...).
fn clean(t: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in t.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.trim().to_string();
    // Alucinaciones frecuentes de whisper con audio casi vacío.
    let n = crate::intents::normalize(&out);
    if n.is_empty()
        || n.starts_with("subtitulos")
        || n.contains("amara org")
        || n == "gracias por ver el video"
    {
        return String::new();
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn cleans_markers() {
        assert_eq!(super::clean(" [Música] Hola Nova (risas)"), "Hola Nova");
        assert_eq!(super::clean("Subtítulos realizados por la comunidad de Amara.org"), "");
    }
}
