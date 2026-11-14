//! Acceso a archivos para Nova (local y Claude), al estilo de las herramientas de Claude:
//! listar, buscar por nombre, buscar texto, leer, escribir, editar y abrir.
//!
//! Límites de seguridad: solo dentro de la carpeta del usuario (y unidades externas en
//! /Volumes), nunca en carpetas de credenciales, y sin borrar nada.

use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

const MAX_READ: usize = 200_000;
const MAX_RESULTS: usize = 200;
const MAX_SEARCH_FILE: u64 = 2_000_000;
const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", "Library", "AppData", ".Trash", "$Recycle.Bin", ".cache", "dist", "build",
];
const SECRET_DIRS: &[&str] = &[".ssh", ".gnupg", ".aws", "Keychains", ".config/gcloud", ".kube", ".docker"];

pub fn tool_definitions() -> Vec<Value> {
    let path = |d: &str| json!({"type": "string", "description": d});
    vec![
        tool(
            "list_dir",
            "Lista lo que hay en una carpeta (archivos y subcarpetas con tamaño). Rutas como \"~/Escritorio\", \"~/Documents\" o absolutas.",
            json!({"path": path("Carpeta. Por defecto, la carpeta del usuario (~).")}),
            &[],
        ),
        tool(
            "find_files",
            "Busca archivos o carpetas por nombre (sin distinguir mayúsculas) dentro de una carpeta y sus subcarpetas.",
            json!({
                "name": {"type": "string", "description": "Parte del nombre o patrón con *, p. ej. \"factura\" o \"*.pdf\"."},
                "path": path("Dónde buscar. Por defecto ~.")
            }),
            &["name"],
        ),
        tool(
            "search_text",
            "Busca un texto dentro del contenido de los archivos de texto de una carpeta. Devuelve archivo, línea y fragmento.",
            json!({
                "query": {"type": "string", "description": "Texto a buscar (sin distinguir mayúsculas)."},
                "path": path("Carpeta o archivo donde buscar. Por defecto ~/Documents.")
            }),
            &["query"],
        ),
        tool(
            "read_file",
            "Lee un archivo de texto (código, notas, .md, .txt, .csv, .json…). Devuelve el contenido con números de línea.",
            json!({"path": path("Ruta del archivo.")}),
            &["path"],
        ),
        tool(
            "write_file",
            "Crea un archivo de texto o lo reemplaza entero con el contenido dado. Crea las carpetas que falten.",
            json!({"path": path("Ruta del archivo."), "content": {"type": "string", "description": "Contenido completo."}}),
            &["path", "content"],
        ),
        tool(
            "edit_file",
            "Cambia un fragmento exacto de un archivo de texto por otro. El fragmento debe aparecer una sola vez.",
            json!({
                "path": path("Ruta del archivo."),
                "old_text": {"type": "string", "description": "Texto exacto que hay ahora."},
                "new_text": {"type": "string", "description": "Texto que lo sustituye."}
            }),
            &["path", "old_text", "new_text"],
        ),
        tool(
            "open_path",
            "Abre un archivo o carpeta con su app predeterminada (p. ej. un PDF en Vista Previa o una carpeta en Finder/Explorador).",
            json!({"path": path("Ruta del archivo o carpeta.")}),
            &["path"],
        ),
    ]
}

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {"type": "object", "properties": props, "required": required}
        }
    })
}

pub fn is_file_tool(name: &str) -> bool {
    matches!(
        name,
        "list_dir" | "find_files" | "search_text" | "read_file" | "write_file" | "edit_file" | "open_path"
    )
}

pub fn describe(name: &str, args: &Value) -> String {
    let p = args["path"].as_str().unwrap_or("~");
    match name {
        "list_dir" => format!("Mirando {p}"),
        "find_files" => format!("Buscando «{}»", args["name"].as_str().unwrap_or("")),
        "search_text" => format!("Buscando el texto «{}»", args["query"].as_str().unwrap_or("")),
        "read_file" => format!("Leyendo {p}"),
        "write_file" => format!("Escribiendo {p}"),
        "edit_file" => format!("Editando {p}"),
        "open_path" => format!("Abriendo {p}"),
        _ => name.into(),
    }
}

pub async fn execute(name: &str, args: &Value) -> String {
    let name = name.to_string();
    let args = args.clone();
    let r = tokio::task::spawn_blocking(move || run(&name, &args))
        .await
        .map_err(|e| anyhow!("{e}"))
        .and_then(|r| r);
    match r {
        Ok(s) => s,
        Err(e) => format!("Error: {e:#}"),
    }
}

fn run(name: &str, args: &Value) -> Result<String> {
    let s = |k: &str| args[k].as_str().unwrap_or_default();
    match name {
        "list_dir" => list_dir(&resolve(args["path"].as_str().unwrap_or("~"))?),
        "find_files" => find_files(&resolve(args["path"].as_str().unwrap_or("~"))?, s("name")),
        "search_text" => search_text(&resolve(args["path"].as_str().unwrap_or("~/Documents"))?, s("query")),
        "read_file" => read_file(&resolve(s("path"))?),
        "write_file" => write_file(&resolve(s("path"))?, s("content")),
        "edit_file" => edit_file(&resolve(s("path"))?, s("old_text"), s("new_text")),
        "open_path" => open_path(&resolve(s("path"))?),
        _ => bail!("herramienta desconocida"),
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Nombres en español de las carpetas habituales (el disco las guarda en inglés).
fn alias(first: &str) -> Option<&'static str> {
    Some(match crate::intents::normalize(first).as_str() {
        "escritorio" => "Desktop",
        "documentos" => "Documents",
        "descargas" => "Downloads",
        "imagenes" | "fotos" => "Pictures",
        "musica" => "Music",
        "videos" | "peliculas" => "Movies",
        _ => return None,
    })
}

/// Convierte "~/Escritorio/x" o una ruta relativa en absoluta y comprueba los límites.
pub fn resolve(raw: &str) -> Result<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("falta la ruta");
    }
    let home = home();
    let rel = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")).or(if raw == "~" { Some("") } else { None });
    let mut path = match rel {
        Some(r) => home.join(r),
        None if Path::new(raw).is_absolute() => PathBuf::from(raw),
        None => home.join(raw),
    };
    // «~/Escritorio» → ~/Desktop si la carpeta en español no existe.
    if let Ok(rest) = path.strip_prefix(&home) {
        let mut comps = rest.components();
        if let Some(Component::Normal(first)) = comps.next() {
            if let Some(real) = alias(&first.to_string_lossy()) {
                if !home.join(first).exists() {
                    path = home.join(real).join(comps.as_path());
                }
            }
        }
    }
    let path = normalize_path(&path);
    check_allowed(&path, &home)?;
    Ok(path)
}

fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn check_allowed(path: &Path, home: &Path) -> Result<()> {
    let in_home = path.starts_with(home);
    let external = path.starts_with("/Volumes") || (cfg!(windows) && !path.starts_with("C:\\"));
    if !in_home && !external {
        bail!("solo puedo trabajar dentro de tu carpeta de usuario ({})", home.display());
    }
    let rel = path.strip_prefix(home).unwrap_or(path).to_string_lossy().replace('\\', "/");
    if SECRET_DIRS.iter().any(|d| rel == *d || rel.starts_with(&format!("{d}/")) || rel.contains(&format!("/{d}/")) || rel.ends_with(&format!("/{d}"))) {
        bail!("esa carpeta guarda credenciales y no la toco");
    }
    Ok(())
}

fn human_size(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

fn list_dir(dir: &Path) -> Result<String> {
    let mut entries: Vec<(bool, String, u64)> = std::fs::read_dir(dir)
        .map_err(|e| anyhow!("no puedo abrir {}: {e}", dir.display()))?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| {
            let md = e.metadata().ok();
            let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            (is_dir, e.file_name().to_string_lossy().into_owned(), md.map(|m| m.len()).unwrap_or(0))
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    let total = entries.len();
    let mut out = format!("{} ({total} elementos)\n", dir.display());
    for (is_dir, name, size) in entries.into_iter().take(MAX_RESULTS) {
        if is_dir {
            out.push_str(&format!("[carpeta] {name}/\n"));
        } else {
            out.push_str(&format!("{name} · {}\n", human_size(size)));
        }
    }
    if total > MAX_RESULTS {
        out.push_str(&format!("… y {} más\n", total - MAX_RESULTS));
    }
    Ok(out)
}

/// Coincidencia de nombre: "*.pdf", "informe*", o una parte del nombre.
pub fn name_matches(pattern: &str, name: &str) -> bool {
    let p = pattern.to_lowercase();
    let n = name.to_lowercase();
    if !p.contains('*') {
        return n.contains(&p);
    }
    let parts: Vec<&str> = p.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match n[pos..].find(part) {
            Some(at) => {
                if i == 0 && at != 0 {
                    return false;
                }
                pos += at + part.len();
            }
            None => return false,
        }
    }
    parts.last().map(|l| l.is_empty() || n.ends_with(l)).unwrap_or(true)
}

fn walk(root: &Path, max_depth: usize, f: &mut dyn FnMut(&Path, bool) -> bool) {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let p = e.path();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !f(&p, is_dir) {
                return;
            }
            if is_dir && depth < max_depth && !SKIP_DIRS.contains(&name.as_str()) && !name.ends_with(".app") {
                stack.push((p, depth + 1));
            }
        }
    }
}

fn find_files(root: &Path, pattern: &str) -> Result<String> {
    if pattern.trim().is_empty() {
        bail!("falta el nombre a buscar");
    }
    let mut hits = Vec::new();
    walk(root, 8, &mut |p, is_dir| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name_matches(pattern.trim(), &name) {
            hits.push(format!("{}{}", p.display(), if is_dir { "/" } else { "" }));
        }
        hits.len() < MAX_RESULTS
    });
    if hits.is_empty() {
        return Ok(format!("No he encontrado nada que se llame «{pattern}» en {}", root.display()));
    }
    Ok(format!("{} resultados:\n{}", hits.len(), hits.join("\n")))
}

fn is_texty(p: &Path) -> bool {
    const EXT: &[&str] = &[
        "txt", "md", "markdown", "csv", "tsv", "json", "yaml", "yml", "toml", "xml", "html", "htm", "css", "js", "ts",
        "tsx", "jsx", "rs", "py", "swift", "kt", "java", "c", "h", "cpp", "hpp", "cs", "go", "rb", "php", "sh", "ps1",
        "bat", "ini", "cfg", "conf", "log", "sql", "tex", "rtf", "srt", "vtt", "env.example",
    ];
    p.extension().map(|e| EXT.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

fn search_text(root: &Path, query: &str) -> Result<String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        bail!("falta el texto a buscar");
    }
    let mut hits = Vec::new();
    // Devuelve si hay que seguir buscando.
    let mut check = |p: &Path| -> bool {
        if !is_texty(p) || std::fs::metadata(p).map(|m| m.len() > MAX_SEARCH_FILE).unwrap_or(true) {
            return true;
        }
        let Ok(txt) = std::fs::read_to_string(p) else { return true };
        for (i, line) in txt.lines().enumerate() {
            if line.to_lowercase().contains(&q) {
                let snippet: String = line.trim().chars().take(160).collect();
                hits.push(format!("{}:{}: {snippet}", p.display(), i + 1));
                if hits.len() >= MAX_RESULTS {
                    return false;
                }
            }
        }
        true
    };
    if root.is_file() {
        check(root);
    } else {
        walk(root, 8, &mut |p, is_dir| is_dir || check(p));
    }
    if hits.is_empty() {
        return Ok(format!("No aparece «{query}» en {}", root.display()));
    }
    Ok(format!("{} coincidencias:\n{}", hits.len(), hits.join("\n")))
}

fn read_file(p: &Path) -> Result<String> {
    let bytes = std::fs::read(p).map_err(|e| anyhow!("no puedo leer {}: {e}", p.display()))?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        bail!("{} no es un archivo de texto (puedo abrirlo con open_path)", p.display());
    }
    let txt = String::from_utf8_lossy(&bytes);
    let mut out = String::new();
    for (i, line) in txt.lines().enumerate() {
        out.push_str(&format!("{:>5}  {line}\n", i + 1));
        if out.len() > MAX_READ {
            out.push_str(&format!("… (archivo cortado: {} en total)\n", human_size(bytes.len() as u64)));
            break;
        }
    }
    Ok(format!("{}\n{out}", p.display()))
}

fn write_file(p: &Path, content: &str) -> Result<String> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existed = p.exists();
    std::fs::write(p, content).map_err(|e| anyhow!("no puedo escribir {}: {e}", p.display()))?;
    Ok(format!(
        "{} {} ({} líneas)",
        if existed { "Reemplazado" } else { "Creado" },
        p.display(),
        content.lines().count()
    ))
}

fn edit_file(p: &Path, old: &str, new: &str) -> Result<String> {
    if old.is_empty() {
        bail!("old_text está vacío");
    }
    let txt = std::fs::read_to_string(p).map_err(|e| anyhow!("no puedo leer {}: {e}", p.display()))?;
    match txt.matches(old).count() {
        0 => bail!("ese texto no aparece en {}; léelo antes con read_file y copia el fragmento exacto", p.display()),
        1 => {}
        n => bail!("ese texto aparece {n} veces; incluye más contexto para que sea único"),
    }
    std::fs::write(p, txt.replacen(old, new, 1))?;
    Ok(format!("Editado {}", p.display()))
}

fn open_path(p: &Path) -> Result<String> {
    if !p.exists() {
        bail!("no existe {}", p.display());
    }
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(p).status()?;
    #[cfg(windows)]
    let status = std::process::Command::new("explorer").arg(p).status()?;
    #[cfg(not(any(windows, target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(p).status()?;
    // explorer.exe devuelve 1 aunque funcione.
    if !status.success() && !cfg!(windows) {
        bail!("no se pudo abrir {}", p.display());
    }
    Ok(format!("Abierto {}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(name_matches("*.pdf", "Factura Enero.PDF"));
        assert!(name_matches("factura", "Mi factura.pdf"));
        assert!(name_matches("inf*2026*", "informe_2026_final.docx"));
        assert!(!name_matches("*.pdf", "notas.txt"));
        assert!(!name_matches("inf*", "mi_informe.txt"));
    }

    #[test]
    fn resolves_inside_home_only() {
        let home = home();
        assert_eq!(resolve("~").unwrap(), normalize_path(&home));
        assert!(resolve("/etc/passwd").is_err());
        assert!(resolve("~/../../etc").is_err());
        assert!(resolve("~/.ssh/id_ed25519").is_err());
        assert!(resolve("~/Documents/x.txt").unwrap().starts_with(&home));
    }

    #[test]
    fn write_edit_read_roundtrip() {
        let dir = home().join(format!(".nova-test-{}", std::process::id()));
        let f = dir.join("sub/nota.md");
        let fs = f.to_string_lossy().to_string();
        assert!(run("write_file", &json!({"path": fs, "content": "hola\nmundo\n"})).unwrap().starts_with("Creado"));
        run("edit_file", &json!({"path": fs, "old_text": "mundo", "new_text": "Nova"})).unwrap();
        let r = run("read_file", &json!({"path": fs})).unwrap();
        assert!(r.contains("Nova"));
        assert!(run("edit_file", &json!({"path": fs, "old_text": "nada", "new_text": "x"})).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
