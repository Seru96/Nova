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
