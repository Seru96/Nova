//! Motor local embebido: llama.cpp (`llama-server`) gestionado por Nova.
//!
//! En el primer arranque Nova descarga el runtime de llama.cpp adecuado para la
//! plataforma (Metal en macOS Apple Silicon, Vulkan en Windows) y el modelo GGUF.
//! Después lo lanza como proceso hijo en 127.0.0.1 y habla con él por su API
//! compatible con OpenAI. El usuario no tiene que instalar nada.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::download::{download_file, extract_zip, find_file, http_client};

/// Modelo de lenguaje local por defecto: rápido, buen español y tool-calling.
/// ~2,5 GB en Q4_K_M; cabe holgado en un Mac de 16 GB y en una 3060 Ti (8 GB).
pub const LLM_FILE: &str = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
pub const LLM_URL: &str = "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf";

/// Modelo de reconocimiento de voz (whisper.cpp, multilingüe, ~190 MB).
pub const STT_FILE: &str = "ggml-small-q5_1.bin";
pub const STT_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin";

/// Las builds de llama.cpp (bNNNN) se publican como *prerelease*, así que
/// `/releases/latest` no las devuelve: listamos las recientes y elegimos la primera
/// que tenga paquete para esta plataforma.
const LLAMA_RELEASES: &str = "https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=15";
const CONTEXT_TOKENS: u32 = 8192;

#[cfg(windows)]
const SERVER_BIN: &str = "llama-server.exe";
#[cfg(not(windows))]
const SERVER_BIN: &str = "llama-server";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Downloading,
    Starting,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub phase: Phase,
    pub detail: String,
    /// 0..1 cuando hay una descarga en curso.
    pub progress: Option<f32>,
}

pub struct LocalEngine {
    app: AppHandle,
    dir: PathBuf,
    status: RwLock<EngineStatus>,
    child: Mutex<Option<tokio::process::Child>>,
    port: Mutex<Option<u16>>,
    setup_lock: tokio::sync::Mutex<()>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Resultado de una pasada por el modelo local.
pub enum LocalOutcome {
    Text(String),
    /// El modelo quiere usar herramientas. `content` es el texto que haya dicho antes.
    ToolCalls { content: String, calls: Vec<ToolCall> },
}

#[derive(Debug, Clone, Default)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl LocalEngine {
    pub fn new(app: AppHandle, dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            app,
            dir,
            status: RwLock::new(EngineStatus {
                phase: Phase::Idle,
                detail: "Sin iniciar".into(),
                progress: None,
            }),
            child: Mutex::new(None),
            port: Mutex::new(None),
            setup_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn models_dir(&self) -> PathBuf {
        self.dir.join("models")
    }

    pub fn stt_model_path(&self) -> PathBuf {
        self.models_dir().join(STT_FILE)
    }

    pub fn status(&self) -> EngineStatus {
        self.status.read().clone()
    }

    fn set_status(&self, phase: Phase, detail: impl Into<String>, progress: Option<f32>) {
        let st = EngineStatus {
            phase,
            detail: detail.into(),
            progress,
        };
        *self.status.write() = st.clone();
        let _ = self.app.emit("engine://status", st);
    }

    pub fn is_ready(&self) -> bool {
        self.status.read().phase == Phase::Ready
    }

    /// Descarga lo que falte y arranca llama-server. Idempotente.
    pub async fn setup(self: &Arc<Self>) -> Result<()> {
        let _guard = self.setup_lock.lock().await;
        if self.is_ready() {
            return Ok(());
        }
        match self.setup_inner().await {
            Ok(()) => Ok(()),
            Err(e) => {
                log::error!("motor local: {e:#}");
                self.set_status(Phase::Error, format!("{e:#}"), None);
                Err(e)
            }
        }
    }

    async fn setup_inner(self: &Arc<Self>) -> Result<()> {
        let server = self.ensure_runtime().await?;
        let model = self.models_dir().join(LLM_FILE);
        self.download_with_status(LLM_URL, &model, "Descargando el cerebro local de Nova")
            .await?;
        self.download_with_status(STT_URL, &self.stt_model_path(), "Descargando el oído de Nova")
            .await?;
        self.launch(&server, &model).await
    }

    async fn download_with_status(&self, url: &str, dest: &Path, label: &str) -> Result<()> {
        if dest.exists() {
            return Ok(());
        }
        self.set_status(Phase::Downloading, label, Some(0.0));
        let label = label.to_string();
        download_file(url, dest, |done, total| {
            let progress = total.map(|t| done as f32 / t.max(1) as f32);
            let mb = done as f64 / 1_048_576.0;
            let detail = match total {
                Some(t) => format!("{label} ({mb:.0} / {:.0} MB)", t as f64 / 1_048_576.0),
                None => format!("{label} ({mb:.0} MB)"),
            };
            self.set_status(Phase::Downloading, detail, progress);
        })
        .await
    }

    /// Devuelve la ruta a llama-server, descargándolo si hace falta.
    async fn ensure_runtime(&self) -> Result<PathBuf> {
        if let Ok(custom) = std::env::var("NOVA_LLAMA_SERVER") {
            let p = PathBuf::from(custom);
            if p.exists() {
                return Ok(p);
            }
        }
        let rt_dir = self.dir.join("runtime");
        if let Some(p) = find_file(&rt_dir, SERVER_BIN) {
            return Ok(p);
        }

        self.set_status(Phase::Downloading, "Buscando el motor llama.cpp", None);
        let releases: Value = http_client()
            .get(LLAMA_RELEASES)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let (name, url) = pick_from_releases(&releases)
            .ok_or_else(|| anyhow!("no hay build de llama.cpp para esta plataforma"))?;
        let archive = self.dir.join("downloads").join(&name);
        self.download_with_status(&url, &archive, "Descargando el motor llama.cpp")
            .await?;

        self.set_status(Phase::Starting, "Instalando el motor", None);
        let archive2 = archive.clone();
        let rt2 = rt_dir.clone();
        tokio::task::spawn_blocking(move || extract_archive(&archive2, &rt2)).await??;
        let _ = std::fs::remove_file(&archive);

        let bin = find_file(&rt_dir, SERVER_BIN)
            .ok_or_else(|| anyhow!("el paquete de llama.cpp no contiene {SERVER_BIN}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755));
        }
        Ok(bin)
    }

    async fn launch(&self, server: &Path, model: &Path) -> Result<()> {
        self.stop();
        self.set_status(Phase::Starting, "Despertando a Nova", None);
        let port = free_port()?;
        let log_path = self.dir.join("llama-server.log");
        let log = std::fs::File::create(&log_path)?;

        let mut cmd = tokio::process::Command::new(server);
        cmd.arg("-m")
            .arg(model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["-c", &CONTEXT_TOKENS.to_string()])
            .args(["-ngl", "999"])
            .args(["--parallel", "1"])
            .arg("--jinja")
            .current_dir(server.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

        let child = cmd.spawn().context("no se pudo lanzar llama-server")?;
        *self.child.lock() = Some(child);

        // Esperar a que el modelo esté cargado.
        let client = http_client();
        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = std::time::Instant::now() + Duration::from_secs(240);
        loop {
            if let Some(code) = self.child_exit_code() {
                let tail = std::fs::read_to_string(&log_path).unwrap_or_default();
                let tail: String = tail.lines().rev().take(8).collect::<Vec<_>>().join("\n");
                bail!("llama-server terminó (código {code}). Últimas líneas:\n{tail}");
            }
            if let Ok(r) = client.get(&health).send().await {
                if r.status().is_success() {
                    break;
                }
            }
            if std::time::Instant::now() > deadline {
                bail!("llama-server no respondió a tiempo");
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        *self.port.lock() = Some(port);
        self.set_status(Phase::Ready, "Modelo local listo", None);
        Ok(())
    }

    fn child_exit_code(&self) -> Option<i32> {
        let mut g = self.child.lock();
        let child = g.as_mut()?;
        match child.try_wait() {
            Ok(Some(st)) => Some(st.code().unwrap_or(-1)),
            _ => None,
        }
    }

    pub fn stop(&self) {
        if let Some(mut c) = self.child.lock().take() {
            let _ = c.start_kill();
        }
        *self.port.lock() = None;
    }

    /// Ejecuta una pasada del modelo local con streaming.
    /// `messages` ya incluye el prompt de sistema; `on_delta` recibe cada fragmento.
    pub async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        mut on_delta: impl FnMut(&str),
    ) -> Result<LocalOutcome> {
        let port = (*self.port.lock()).ok_or_else(|| anyhow!("el modelo local no está listo"))?;
        if self.child_exit_code().is_some() {
            bail!("el modelo local se ha detenido; reinícialo desde Ajustes");
        }

        let mut body = json!({
            "messages": messages,
            "stream": true,
            "temperature": 0.7,
            "top_p": 0.8,
            "max_tokens": 1024,
        });
        if !tools.is_empty() {
            body["tools"] = json!(tools);
            body["tool_choice"] = json!("auto");
        }

        let resp = http_client()
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("modelo local: {status} {text}");
        }

        let mut text = String::new();
        let mut calls: Vec<ToolCall> = Vec::new();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        'outer: while let Some(chunk) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(nl) = buf.find('\n') {
                let line: String = buf.drain(..=nl).collect();
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    break 'outer;
                }
                let Ok(v) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                let delta = &v["choices"][0]["delta"];
                if let Some(t) = delta["content"].as_str() {
                    if !t.is_empty() {
                        text.push_str(t);
                        on_delta(t);
                    }
                }
                if let Some(deltas) = delta["tool_calls"].as_array() {
                    for c in deltas {
                        let idx = c["index"].as_u64().unwrap_or(calls.len().saturating_sub(1) as u64) as usize;
                        while calls.len() <= idx {
                            calls.push(ToolCall::default());
                        }
                        let call = &mut calls[idx];
                        if let Some(id) = c["id"].as_str() {
                            call.id = id.to_string();
                        }
                        if let Some(n) = c["function"]["name"].as_str() {
                            call.name.push_str(n);
                        }
                        if let Some(a) = c["function"]["arguments"].as_str() {
                            call.arguments.push_str(a);
                        }
                    }
                }
            }
        }

        calls.retain(|c| !c.name.is_empty());
        if calls.is_empty() {
            return Ok(LocalOutcome::Text(text));
        }
        for (i, c) in calls.iter_mut().enumerate() {
            if c.id.is_empty() {
                c.id = format!("call_{i}");
            }
        }
        Ok(LocalOutcome::ToolCalls { content: text, calls })
    }
}

pub fn escalate_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "escalate_to_claude",
            "description": "Delega la petición a un modelo mucho más potente (Claude) cuando requiere código complejo, arquitectura de software, razonamiento profundo o supera tu capacidad.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "La tarea a resolver, autocontenida."},
                    "reason": {"type": "string", "description": "Por qué necesitas escalar, en pocas palabras."}
                },
                "required": ["task", "reason"]
            }
        }
    })
}

/// Recorre las releases (de más nueva a más antigua) y devuelve (nombre, url)
/// del primer paquete compatible con esta plataforma.
fn pick_from_releases(releases: &Value) -> Option<(String, String)> {
    releases.as_array()?.iter().find_map(|r| {
        let assets: Vec<(&str, &str)> = r["assets"]
            .as_array()?
            .iter()
            .filter_map(|a| Some((a["name"].as_str()?, a["browser_download_url"].as_str()?)))
            .collect();
        pick_runtime_asset(&assets).map(|(n, u)| (n.to_string(), u.to_string()))
    })
}

/// Elige el paquete de llama.cpp para esta plataforma entre los assets de la release.
fn pick_runtime_asset<'a>(assets: &[(&'a str, &'a str)]) -> Option<(&'a str, &'a str)> {
    let prefs: &[&str] = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        &["bin-macos-arm64"]
    } else if cfg!(target_os = "macos") {
        &["bin-macos-x64"]
    } else if cfg!(windows) {
        // Vulkan funciona en cualquier GPU (incluida la RTX 3060 Ti) sin instalar CUDA.
        &["bin-win-vulkan-x64", "bin-win-cpu-x64"]
    } else {
        &["bin-ubuntu-vulkan-x64", "bin-ubuntu-x64"]
    };
    for p in prefs {
        if let Some(found) = assets.iter().find(|(n, _)| {
            n.contains(p) && (n.ends_with(".zip") || n.ends_with(".tar.gz"))
        }) {
            return Some(*found);
        }
    }
    None
}

fn extract_archive(archive: &Path, out: &Path) -> Result<()> {
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") {
        extract_zip(archive, out)
    } else {
        let f = std::fs::File::open(archive)?;
        let gz = flate2::read::GzDecoder::new(f);
        tar::Archive::new(gz).unpack(out)?;
        Ok(())
    }
}

fn free_port() -> Result<u16> {
    let l = TcpListener::bind("127.0.0.1:0")?;
    Ok(l.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_releases_without_binaries() {
        let releases = json!([
            {"tag_name": "v0.7.0", "assets": [{"name": "nightly-tag.txt", "browser_download_url": "x"}]},
            {"tag_name": "b11785", "assets": [
                {"name": "llama-b11785-bin-macos-arm64.tar.gz", "browser_download_url": "mac"},
                {"name": "llama-b11785-bin-win-vulkan-x64.zip", "browser_download_url": "win"},
                {"name": "llama-b11785-bin-ubuntu-x64.tar.gz", "browser_download_url": "linux"}
            ]}
        ]);
        let (name, _) = pick_from_releases(&releases).unwrap();
        assert!(name.starts_with("llama-b11785-bin-"));
    }

    #[test]
    fn picks_platform_asset() {
        let assets = [
            ("llama-b7000-bin-macos-arm64.zip", "u1"),
            ("llama-b7000-bin-win-vulkan-x64.zip", "u2"),
            ("llama-b7000-bin-win-cpu-x64.zip", "u3"),
            ("llama-b7000-bin-ubuntu-x64.tar.gz", "u4"),
            ("cudart-llama-bin-win-cuda-12.4-x64.zip", "u5"),
        ];
        let picked = pick_runtime_asset(&assets).unwrap();
        if cfg!(windows) {
            assert_eq!(picked.1, "u2");
        } else if cfg!(target_os = "macos") {
            assert_eq!(picked.1, "u1");
        } else {
            assert_eq!(picked.1, "u4");
        }
    }
}
