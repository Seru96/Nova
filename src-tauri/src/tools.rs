//! Todas las herramientas que Nova puede usar, en un solo sitio, para el modelo local y
//! para Claude: control de apps, archivos y skills.

use serde_json::Value;
use tauri::AppHandle;

use crate::settings::Settings;
use crate::skills::Skills;
use crate::{control, files, skills};

pub fn definitions(s: &Settings) -> Vec<Value> {
    let mut t = vec![skills::tool_definition()];
    if s.app_control {
        t.extend(control::tool_definitions());
    }
    if s.file_access {
        t.extend(files::tool_definitions());
    }
    t
}

pub fn describe(name: &str, args: &Value) -> String {
    if name == "use_skill" {
        return format!("Consultando la skill {}", args["name"].as_str().unwrap_or(""));
    }
    if files::is_file_tool(name) {
        return files::describe(name, args);
    }
    control::describe(name, args)
}

/// ¿Se enseña en el chat? (`use_skill` es interna).
pub fn visible(name: &str) -> bool {
    name != "use_skill"
}

pub async fn execute(app: &AppHandle, skills: &Skills, s: &Settings, name: &str, args: &Value) -> String {
    if name == "use_skill" {
        let n = args["name"].as_str().unwrap_or_default();
        return match skills.get(n) {
            Some(sk) => sk.body,
            None => {
                let names: Vec<String> = skills.all().into_iter().map(|s| s.name).collect();
                format!("No hay ninguna skill «{n}». Disponibles: {}", names.join(", "))
            }
        };
    }
    if control::is_control_tool(name) {
        if !s.app_control {
            return "Error: el control de apps está desactivado en Ajustes".into();
        }
        return control::execute(app, name, args).await;
    }
    if files::is_file_tool(name) {
        if !s.file_access {
            return "Error: el acceso a archivos está desactivado en Ajustes".into();
        }
        return files::execute(name, args).await;
    }
    "Error: herramienta no disponible".into()
}

