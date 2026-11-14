//! Acceso a teclado y ratón (enigo) con el aviso de Accesibilidad de macOS una sola vez.
//!
//! `Enigo::new` con los ajustes por defecto vuelve a mostrar el diálogo de permisos en
//! cada intento si falta el permiso. H.A.L.O. crea conexiones muy a menudo, así que solo
//! lo mostramos la primera vez y después comprobamos en silencio.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, Result};
use enigo::{Enigo, Settings};

static PROMPTED: AtomicBool = AtomicBool::new(false);

pub const HINT: &str = "Activa Nova en Ajustes del Sistema → Privacidad y seguridad → Accesibilidad. \
Si ya aparece activada, quítala con «−», vuelve a añadirla y reinicia Nova (cada versión nueva lo necesita).";

pub fn enigo() -> Result<Enigo> {
    let first = !PROMPTED.swap(true, Ordering::SeqCst);
    let settings = Settings {
        open_prompt_to_get_permissions: first,
        ..Settings::default()
    };
    Enigo::new(&settings).map_err(|e| {
        if cfg!(target_os = "macos") {
            anyhow!("no tengo permiso para controlar el ordenador ({e}). {HINT}")
        } else {
            anyhow!("no puedo controlar el teclado ni el ratón: {e}")
        }
    })
}

static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

pub fn init(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

/// Ejecuta el teclado en el hilo principal en macOS: enigo consulta la distribución del
/// teclado (Text Input Sources) y macOS cierra la app si eso pasa fuera del hilo principal.
/// Hay que llamarlo desde un hilo secundario (p. ej. `spawn_blocking`).
pub fn run_keyboard<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    #[cfg(target_os = "macos")]
    {
        let app = APP.get().ok_or_else(|| anyhow!("la app aún no está lista"))?;
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(f());
        })
        .map_err(|e| anyhow!("{e}"))?;
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| anyhow!("el teclado no respondió"))?
    }
    #[cfg(not(target_os = "macos"))]
    {
        f()
    }
}
