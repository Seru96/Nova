//! Control de otras aplicaciones: abrirlas, escribir en ellas y pulsar atajos.
//!
//! - macOS: `open -a` para abrir/activar y eventos de teclado (CGEvent vía enigo).
//!   La primera vez macOS pide el permiso de Accesibilidad.
//! - Windows: accesos directos del menú Inicio para abrir y SendInput (vía enigo).

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde_json::{json, Value};
use tauri::AppHandle;
#[cfg(not(target_os = "macos"))]
use tauri::Manager;

#[cfg(any(windows, target_os = "macos"))]
use crate::intents::normalize;

/// Definiciones de herramientas (formato OpenAI) para el modelo local.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "open_app",
                "description": "Abre una aplicación del ordenador o la trae al frente si ya está abierta.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "app": {"type": "string", "description": "Nombre de la app, p. ej. \"Safari\", \"Claude\", \"Notas\", \"Spotify\"."}
                    },
                    "required": ["app"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "type_text",
                "description": "Escribe texto con el teclado en una app (o en la que el usuario estaba usando si no se indica). Úsala cuando el usuario pida escribir, redactar o mandar algo en otra app.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": {"type": "string", "description": "Texto exacto a escribir."},
                        "app": {"type": "string", "description": "App donde escribir (opcional)."},
                        "submit": {"type": "boolean", "description": "Pulsar Intro al terminar para enviar."}
                    },
                    "required": ["text"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "press_keys",
                "description": "Pulsa un atajo de teclado, p. ej. \"cmd+t\", \"ctrl+shift+n\", \"enter\", \"esc\".",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "keys": {"type": "string", "description": "Combinación separada por +."},
                        "app": {"type": "string", "description": "App a la que enviarlo (opcional)."}
                    },
                    "required": ["keys"]
                }
            }
        }),
    ]
}

pub fn is_control_tool(name: &str) -> bool {
    matches!(name, "open_app" | "type_text" | "press_keys")
}

/// Texto corto para enseñar en el chat mientras se ejecuta la acción.
pub fn describe(name: &str, args: &Value) -> String {
    let app = args["app"].as_str().unwrap_or("");
    match name {
        "open_app" => format!("Abriendo {app}"),
        "type_text" if !app.is_empty() => format!("Escribiendo en {app}"),
        "type_text" => "Escribiendo en la app activa".into(),
        "press_keys" => format!("Pulsando {}", args["keys"].as_str().unwrap_or("")),
        _ => name.to_string(),
    }
}

/// Ejecuta una herramienta y devuelve el resultado como texto para el modelo.
pub async fn execute(app: &AppHandle, name: &str, args: &Value) -> String {
    let result = match name {
        "open_app" => match args["app"].as_str() {
            Some(a) => open_app(a).await.map(|n| format!("Abierta: {n}")),
            None => Err(anyhow!("falta el nombre de la app")),
        },
        "type_text" => type_text(app, args).await,
        "press_keys" => press_keys(app, args).await,
        _ => Err(anyhow!("herramienta desconocida")),
    };
    match result {
        Ok(msg) => msg,
        Err(e) => format!("Error: {e:#}"),
    }
}

async fn focus_target(app: &AppHandle, target: Option<&str>) -> Result<()> {
    match target.filter(|t| !t.trim().is_empty()) {
        Some(t) => {
            open_app(t).await?;
            tokio::time::sleep(Duration::from_millis(900)).await;
        }
        None => {
            hide_nova(app);
            tokio::time::sleep(Duration::from_millis(450)).await;
        }
    }
    Ok(())
}

async fn type_text(app: &AppHandle, args: &Value) -> Result<String> {
    let text = args["text"].as_str().unwrap_or_default().to_string();
    if text.is_empty() {
        bail!("no hay texto que escribir");
    }
    let submit = args["submit"].as_bool().unwrap_or(false);
    focus_target(app, args["app"].as_str()).await?;
    let n = text.chars().count();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut e = enigo()?;
        e.text(&text).map_err(|e| anyhow!("{e}"))?;
        if submit {
            std::thread::sleep(Duration::from_millis(120));
            e.key(Key::Return, Direction::Click).map_err(|e| anyhow!("{e}"))?;
        }
        Ok(())
    })
    .await??;
    Ok(format!("Escritos {n} caracteres{}", if submit { " y enviado" } else { "" }))
}

async fn press_keys(app: &AppHandle, args: &Value) -> Result<String> {
    let combo = args["keys"].as_str().unwrap_or_default().to_string();
    let (mods, key) = parse_combo(&combo)?;
    focus_target(app, args["app"].as_str()).await?;
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut e = enigo()?;
        for m in &mods {
            e.key(*m, Direction::Press).map_err(|e| anyhow!("{e}"))?;
        }
        let r = e.key(key, Direction::Click);
        for m in mods.iter().rev() {
            let _ = e.key(*m, Direction::Release);
        }
        r.map_err(|e| anyhow!("{e}"))
    })
    .await??;
    Ok(format!("Pulsado {combo}"))
}

fn enigo() -> Result<Enigo> {
    Enigo::new(&Settings::default()).map_err(|e| {
        anyhow!("no puedo controlar el teclado ({e}). En macOS, activa Nova en Ajustes del Sistema → Privacidad y seguridad → Accesibilidad")
    })
}

/// Convierte "cmd+shift+t" en modificadores + tecla.
pub fn parse_combo(combo: &str) -> Result<(Vec<Key>, Key)> {
    let parts: Vec<String> = combo
        .split('+')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let (last, mods) = parts.split_last().ok_or_else(|| anyhow!("atajo vacío"))?;
    let mut keys = Vec::new();
    for m in mods {
        keys.push(match m.as_str() {
            "cmd" | "command" | "meta" | "win" | "super" | "⌘" => Key::Meta,
            "ctrl" | "control" | "ctl" => Key::Control,
            "alt" | "option" | "opt" | "⌥" => Key::Alt,
            "shift" | "mayus" | "mayús" | "⇧" => Key::Shift,
            other => bail!("modificador desconocido: {other}"),
        });
    }
    let key = match last.as_str() {
        "enter" | "return" | "intro" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "space" | "espacio" => Key::Space,
        "backspace" | "borrar" => Key::Backspace,
        "up" | "arriba" => Key::UpArrow,
        "down" | "abajo" => Key::DownArrow,
        "left" | "izquierda" => Key::LeftArrow,
        "right" | "derecha" => Key::RightArrow,
        s if s.chars().count() == 1 => Key::Unicode(s.chars().next().unwrap_or(' ')),
        other => bail!("tecla desconocida: {other}"),
    };
    Ok((keys, key))
}

/// Oculta Nova para que la app que el usuario usaba antes quede al frente.
fn hide_nova(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.hide();
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
}

#[cfg(target_os = "macos")]
async fn open_app(name: &str) -> Result<String> {
    let name = name.trim().to_string();
    let ok = tokio::process::Command::new("open")
        .args(["-a", &name])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        return Ok(name);
    }
    // Búsqueda aproximada en las carpetas de aplicaciones (p. ej. "notas" → Notes.app).
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = [
        "/Applications".to_string(),
        "/System/Applications".to_string(),
        "/System/Applications/Utilities".to_string(),
        format!("{home}/Applications"),
    ];
    let wanted = normalize(&name);
    let aliases = mac_aliases(&wanted);
    for d in dirs {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            let Some(stem) = file.strip_suffix(".app") else { continue };
            let n = normalize(stem);
            if n.contains(&wanted) || aliases.iter().any(|a| n == *a) {
                let path = e.path();
                let st = tokio::process::Command::new("open").arg(&path).status().await?;
                if st.success() {
                    return Ok(stem.to_string());
                }
            }
        }
    }
    bail!("no encuentro la app «{name}»")
}

/// Nombres en español de apps del sistema cuyo bundle está en inglés.
#[cfg(target_os = "macos")]
fn mac_aliases(n: &str) -> Vec<&'static str> {
    match n {
        "notas" => vec!["notes"],
        "calendario" => vec!["calendar"],
        "recordatorios" => vec!["reminders"],
        "mensajes" => vec!["messages"],
        "musica" => vec!["music"],
        "fotos" => vec!["photos"],
        "correo" | "mail" => vec!["mail"],
        "ajustes" | "ajustes del sistema" | "preferencias" => vec!["system settings"],
        "calculadora" => vec!["calculator"],
        "terminal" => vec!["terminal"],
        "mapas" => vec!["maps"],
        "contactos" => vec!["contacts"],
        _ => vec![],
    }
}

#[cfg(windows)]
async fn open_app(name: &str) -> Result<String> {
    let name = name.trim().to_string();
    let wanted = normalize(&name);
    let mut roots = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Ok(base) = std::env::var(var) {
            roots.push(std::path::PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs"));
        }
    }
    let mut best: Option<(usize, std::path::PathBuf, String)> = None;
    for r in roots {
        walk_lnk(&r, &mut |p| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let n = normalize(&stem);
            if n.contains(&wanted) {
                let score = n.len(); // el nombre más corto que encaje suele ser el bueno
                if best.as_ref().map_or(true, |(s, _, _)| score < *s) {
                    best = Some((score, p.to_path_buf(), stem));
                }
            }
        });
    }
    let (target, label) = match best {
        Some((_, p, stem)) => (p.to_string_lossy().to_string(), stem),
        None => (name.clone(), name.clone()), // apps en PATH: notepad, chrome...
    };
    let mut cmd = tokio::process::Command::new("cmd");
    cmd.args(["/C", "start", "", &target]).creation_flags(0x0800_0000);
    let st = cmd.status().await?;
    if !st.success() {
        bail!("no encuentro la app «{name}»");
    }
    Ok(label)
}

#[cfg(windows)]
fn walk_lnk(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_lnk(&p, f);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            f(&p);
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
async fn open_app(name: &str) -> Result<String> {
    let st = tokio::process::Command::new("xdg-open").arg(name).status().await?;
    if !st.success() {
        bail!("no encuentro la app «{name}»");
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos() {
        let (m, k) = parse_combo("cmd+shift+t").unwrap();
        assert_eq!(m, vec![Key::Meta, Key::Shift]);
        assert_eq!(k, Key::Unicode('t'));
        let (m, k) = parse_combo("Enter").unwrap();
        assert!(m.is_empty());
        assert_eq!(k, Key::Return);
        assert!(parse_combo("hyper+x").is_err());
    }
}
