//! Descargas de primer arranque (modelos y runtime) con reanudación y progreso.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("nova-desktop/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("cliente HTTP")
}

/// Descarga `url` a `dest`. Usa `dest.part` para poder reanudar si se corta.
/// `progress(descargado, total)` se llama periódicamente.
pub async fn download_file(
    url: &str,
    dest: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    if dest.exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part: PathBuf = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("bin")
    ));
    let already = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    let client = http_client();
    let mut req = client.get(url);
    if already > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={already}-"));
    }
    let resp = req.send().await.with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("descarga fallida ({status}) para {url}");
    }
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let start = if resumed { already } else { 0 };
    let total = resp.content_length().map(|l| l + start);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .await?;

    let mut done = start;
    let mut last_report = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("conexión interrumpida durante la descarga")?;
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        if done - last_report > 512 * 1024 {
            last_report = done;
            progress(done, total);
        }
    }
    file.flush().await?;
    drop(file);
    if let Some(t) = total {
        if done < t {
            bail!("descarga incompleta ({done}/{t} bytes)");
        }
    }
    progress(done, total);
    tokio::fs::rename(&part, dest).await?;
    Ok(())
}

/// Extrae un .zip completo en `out_dir` (bloqueante; llamar con spawn_blocking).
pub fn extract_zip(zip_path: &Path, out_dir: &Path) -> Result<()> {
    let f = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(f)?;
    std::fs::create_dir_all(out_dir)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out = out_dir.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut w = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut w)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

/// Busca recursivamente un fichero por nombre.
pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(p);
        }
    }
    subdirs.into_iter().find_map(|d| find_file(&d, name))
}
