//! Skills de Nova: recetas en Markdown con el mismo formato que las skills de Claude
//! (`<carpeta>/SKILL.md` con `name` y `description` en el encabezado).
//!
//! Viven en `<config>/skills/`: Nova instala ahí las suyas la primera vez y el usuario
//! puede editarlas o añadir otras. Como el modelo local es pequeño, además de ofrecerle
//! la herramienta `use_skill`, metemos directamente en el prompt la receta que encaja con
//! la petición (por las palabras de `keywords`).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::intents::normalize;

const BUILTIN: &[(&str, &str)] = &[
    ("abrir-y-escribir", include_str!("../skills/abrir-y-escribir/SKILL.md")),
    ("correo", include_str!("../skills/correo/SKILL.md")),
    ("buscar-en-internet", include_str!("../skills/buscar-en-internet/SKILL.md")),
    ("musica-y-videos", include_str!("../skills/musica-y-videos/SKILL.md")),
    ("archivos", include_str!("../skills/archivos/SKILL.md")),
    ("notas", include_str!("../skills/notas/SKILL.md")),
];

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub body: String,
}

pub fn parse(text: &str, fallback_name: &str) -> Skill {
    let mut name = fallback_name.to_string();
    let mut description = String::new();
    let mut keywords = Vec::new();
    let mut body = text;
    if let Some(rest) = text.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                let Some((k, v)) = line.split_once(':') else { continue };
                let v = v.trim().trim_matches('"').to_string();
                match k.trim() {
                    "name" => name = v,
                    "description" => description = v,
                    "keywords" => keywords = v.split(',').map(|w| normalize(w.trim())).filter(|w| !w.is_empty()).collect(),
                    _ => {}
                }
            }
            body = rest[end + 4..].trim_start_matches(['-', '\n', '\r']);
        }
    }
    Skill {
        name,
        description,
        keywords,
        body: body.trim().to_string(),
    }
}

pub struct Skills {
    dir: PathBuf,
}

impl Skills {
    /// Prepara la carpeta e instala las skills de serie que falten.
    pub fn new(config_dir: &Path) -> Self {
        let dir = config_dir.join("skills");
        for (name, text) in BUILTIN {
            let d = dir.join(name);
            let f = d.join("SKILL.md");
            if !f.exists() {
                let _ = std::fs::create_dir_all(&d);
                let _ = std::fs::write(&f, text);
            }
        }
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn all(&self) -> Vec<Skill> {
        let mut out: Vec<Skill> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let folder = e.file_name().to_string_lossy().into_owned();
                        let text = std::fs::read_to_string(e.path().join("SKILL.md")).ok()?;
                        Some(parse(&text, &folder))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if out.is_empty() {
            out = BUILTIN.iter().map(|(n, t)| parse(t, n)).collect();
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn get(&self, name: &str) -> Option<Skill> {
        let n = normalize(name);
        self.all().into_iter().find(|s| normalize(&s.name) == n)
    }
}

/// Las skills que encajan con la petición (como mucho dos).
pub fn matching(skills: &[Skill], request: &str) -> Vec<Skill> {
    let text = format!(" {} ", normalize(request));
    let mut scored: Vec<(usize, &Skill)> = skills
        .iter()
        .map(|s| {
            let score = s
                .keywords
                .iter()
                .filter(|k| text.contains(&format!(" {k} ")) || (k.contains(' ') && text.contains(k.as_str())))
                .count();
            (score, s)
        })
        .filter(|(n, _)| *n > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(2).map(|(_, s)| s.clone()).collect()
}

pub fn tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "use_skill",
            "description": "Lee las instrucciones paso a paso de una de tus skills antes de hacer una tarea.",
            "parameters": {
                "type": "object",
                "properties": {"name": {"type": "string", "description": "Nombre de la skill."}},
                "required": ["name"]
            }
        }
    })
}

/// Bloque para el prompt de sistema: la lista de skills y la receta que encaja.
pub fn prompt_section(all: &[Skill], matched: &[Skill]) -> String {
    let mut p = String::from("\n\nTus skills (recetas para tareas; léelas con `use_skill` si te sirven):");
    for s in all {
        p.push_str(&format!("\n- {}: {}", s.name, s.description));
    }
    for s in matched {
        p.push_str(&format!("\n\nInstrucciones de la skill «{}» para esta petición:\n{}", s.name, s.body));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_skills_parse() {
        let all: Vec<Skill> = BUILTIN.iter().map(|(n, t)| parse(t, n)).collect();
        for s in &all {
            assert!(!s.description.is_empty(), "{} sin descripción", s.name);
            assert!(!s.keywords.is_empty(), "{} sin keywords", s.name);
            assert!(s.body.starts_with('#'), "{} cuerpo: {}", s.name, &s.body[..20]);
        }
        let m = matching(&all, "Mándale un correo a Ana diciendo que llego tarde");
        assert_eq!(m[0].name, "correo");
        let m = matching(&all, "escribe en WhatsApp que voy de camino");
        assert_eq!(m[0].name, "abrir-y-escribir");
        let m = matching(&all, "¿qué hay en el escritorio?");
        assert_eq!(m[0].name, "archivos");
        let m = matching(&all, "pon música de Rosalía en Spotify");
        assert_eq!(m[0].name, "musica-y-videos");
        assert!(matching(&all, "¿cuánto es 2 + 2?").is_empty());
    }
}
