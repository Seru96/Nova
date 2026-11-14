//! Detección de órdenes especiales en texto o voz (sin modelo, determinista).

/// Minúsculas, sin tildes y sin signos de puntuación; espacios simples.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PrivacyCommand {
    Enable,
    Disable,
}

const DISABLE: &[&str] = &[
    "desactiva el modo privado",
    "desactiva el modo confidencial",
    "desactivar modo privado",
    "desactivar modo confidencial",
    "desactiva modo privado",
    "desactiva modo confidencial",
    "sal del modo privado",
    "sal del modo confidencial",
    "salir del modo privado",
    "salir del modo confidencial",
    "quita el modo privado",
    "quita el modo confidencial",
    "fin del modo privado",
    "fin del modo confidencial",
    "ya no es confidencial",
];

const ENABLE: &[&str] = &[
    "esto es confidencial",
    "es confidencial",
    "modo privado",
    "modo confidencial",
];

pub fn privacy_command(text: &str) -> Option<PrivacyCommand> {
    let n = normalize(text);
    if DISABLE.iter().any(|p| n.contains(p)) {
        return Some(PrivacyCommand::Disable);
    }
    if ENABLE.iter().any(|p| n.contains(p)) {
        return Some(PrivacyCommand::Enable);
    }
    None
}

/// ¿El mensaje es solo la orden (sin más petición detrás)?
pub fn is_bare_command(text: &str) -> bool {
    normalize(text).split(' ').count() <= 6
}

/// El usuario pide explícitamente que responda Claude.
pub fn wants_claude(text: &str) -> bool {
    let n = normalize(text);
    n.starts_with("claude ")
        || [
            "pregunta a claude",
            "preguntale a claude",
            "pasaselo a claude",
            "usa claude",
            "consulta a claude",
            "consultale a claude",
        ]
        .iter()
        .any(|p| n.contains(p))
}

/// Resultado de buscar la wake word en una transcripción.
#[derive(Debug, PartialEq, Eq)]
pub enum Wake {
    None,
    /// Se dijo "Oye Nova" sin nada más: esperar la orden.
    Bare,
    /// "Oye Nova, <orden>".
    WithCommand(String),
}

fn is_nova_word(w: &str) -> bool {
    matches!(w, "nova" | "noba" | "novah" | "nobba" | "nora")
}

/// Detecta "Oye Nova" / "Nova" en una transcripción de whisper.
pub fn detect_wake(transcript: &str) -> Wake {
    let n = normalize(transcript);
    let words: Vec<&str> = n.split(' ').filter(|w| !w.is_empty()).collect();
    // "nora" solo cuenta si va precedida de "oye" (evita falsos positivos).
    let pos = words.iter().enumerate().position(|(i, w)| {
        if *w == "nora" {
            i > 0 && matches!(words[i - 1], "oye" | "olle" | "oie")
        } else {
            is_nova_word(w)
        }
    });
    let Some(i) = pos else {
        return Wake::None;
    };
    // Buscamos el texto original tras la wake word para conservar tildes y mayúsculas.
    let rest = original_after_word(transcript, i + 1);
    let rest_norm = normalize(&rest);
    if rest_norm.is_empty() {
        Wake::Bare
    } else {
        Wake::WithCommand(rest.trim().trim_start_matches([',', '.', ':', ';', '!', '?', ' ']).to_string())
    }
}

/// Devuelve el texto original que sigue a las primeras `n` palabras.
fn original_after_word(original: &str, n: usize) -> String {
    if n == 0 {
        return original.to_string();
    }
    let mut count = 0;
    let mut in_word = false;
    for (idx, c) in original.char_indices() {
        let alnum = c.is_alphanumeric();
        if alnum && !in_word {
            count += 1;
        }
        if !alnum && in_word && count == n {
            return original[idx..].to_string();
        }
        in_word = alnum;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize("¡Oye, Nóva!  ¿Qué tal?"), "oye nova que tal");
    }

    #[test]
    fn privacy() {
        assert_eq!(privacy_command("Oye, esto es confidencial"), Some(PrivacyCommand::Enable));
        assert_eq!(privacy_command("activa el modo privado"), Some(PrivacyCommand::Enable));
        assert_eq!(privacy_command("desactiva el modo privado"), Some(PrivacyCommand::Disable));
        assert_eq!(privacy_command("¿qué hora es?"), None);
    }

    #[test]
    fn claude_requests() {
        assert!(wants_claude("Pregúntale a Claude cómo hacer X"));
        assert!(wants_claude("Claude, diseña una API"));
        assert!(!wants_claude("hola nova"));
    }

    #[test]
    fn wake_word() {
        assert_eq!(detect_wake("Oye Nova."), Wake::Bare);
        assert_eq!(
            detect_wake("Oye, Nova, ¿qué tiempo hace?"),
            Wake::WithCommand("¿qué tiempo hace?".into())
        );
        assert_eq!(detect_wake("Hoy hace sol"), Wake::None);
        assert_eq!(detect_wake("oye nora pon música"), Wake::WithCommand("pon música".into()));
        assert_eq!(detect_wake("la señora nora"), Wake::None);
    }
}
