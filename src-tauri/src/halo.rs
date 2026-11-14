//! H.A.L.O.: control del ordenador con gestos de la mano.
//!
//! La visión (cámara + 21 puntos por mano + máquina de estados de gestos) vive en la
//! ventana HUD transparente (`overlay`), que nunca se oculta y por eso no la frena el
//! sistema. Este módulo crea esa ventana y ejecuta lo que los gestos piden: mover el
//! ratón, clicar/arrastrar, colocar ventanas y cambiar de escritorio.

use std::sync::mpsc::{channel, Sender};

use anyhow::{anyhow, Result};
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings as EnigoSettings};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::window_manager::{self, DesktopDir, WindowAction, WindowManager};

pub const OVERLAY: &str = "overlay";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonAction {
    Down,
    Up,
    Click,
    RightClick,
}

enum Cmd {
    /// Posición normalizada 0..1 dentro de la pantalla principal.
    Move(f64, f64),
    Button(ButtonAction),
}

/// Hilo dedicado al ratón: mantiene un único `Enigo` vivo (crearlo en cada
/// movimiento es caro en macOS) y no bloquea los comandos de Tauri.
pub struct Pointer {
    tx: Sender<Cmd>,
}

impl Pointer {
    pub fn new() -> Self {
        let (tx, rx) = channel::<Cmd>();
        std::thread::Builder::new()
            .name("halo-pointer".into())
            .spawn(move || {
                let mut enigo: Option<Enigo> = None;
                let mut screen = (0i32, 0i32);
                while let Ok(mut cmd) = rx.recv() {
                    // Si se acumulan movimientos, solo cuenta el último.
                    if matches!(cmd, Cmd::Move(..)) {
                        while let Ok(next) = rx.try_recv() {
                            let is_move = matches!(next, Cmd::Move(..));
                            cmd = next;
                            if !is_move {
                                break;
                            }
                        }
                    }
                    if enigo.is_none() {
                        match Enigo::new(&EnigoSettings::default()) {
                            Ok(e) => {
                                screen = e.main_display().unwrap_or((1440, 900));
                                enigo = Some(e);
                            }
                            Err(e) => {
                                log::warn!("H.A.L.O. sin control del ratón: {e}");
                                continue;
                            }
                        }
                    }
                    let e = enigo.as_mut().expect("enigo creado");
                    let r = match cmd {
                        Cmd::Move(x, y) => {
                            let px = (x.clamp(0.0, 1.0) * (screen.0 - 1) as f64).round() as i32;
                            let py = (y.clamp(0.0, 1.0) * (screen.1 - 1) as f64).round() as i32;
                            e.move_mouse(px, py, Coordinate::Abs)
                        }
                        Cmd::Button(b) => match b {
                            ButtonAction::Down => e.button(Button::Left, Direction::Press),
                            ButtonAction::Up => e.button(Button::Left, Direction::Release),
                            ButtonAction::Click => e.button(Button::Left, Direction::Click),
                            ButtonAction::RightClick => e.button(Button::Right, Direction::Click),
                        },
                    };
                    if let Err(err) = r {
                        log::warn!("H.A.L.O.: {err}");
                    }
                }
            })
            .expect("hilo del puntero");
        Self { tx }
    }

    pub fn move_to(&self, x: f64, y: f64) {
        let _ = self.tx.send(Cmd::Move(x, y));
    }

    pub fn button(&self, b: ButtonAction) {
        let _ = self.tx.send(Cmd::Button(b));
    }
}

/// Muestra (creándola si hace falta) la ventana HUD a pantalla completa.
pub fn show_overlay(app: &AppHandle) -> Result<WebviewWindow> {
    let win = match app.get_webview_window(OVERLAY) {
        Some(w) => w,
        None => {
            let w = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("index.html#overlay".into()))
                .title("Nova H.A.L.O.")
                .transparent(true)
                .decorations(false)
                .shadow(false)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible_on_all_workspaces(true)
                .focused(false)
                .visible(false)
                .build()
                .map_err(|e| anyhow!("no pude crear el HUD de H.A.L.O.: {e}"))?;
            w
        }
    };
    if let Ok(Some(m)) = win.primary_monitor() {
        let _ = win.set_position(*m.position());
        let _ = win.set_size(*m.size());
    }
    win.show().map_err(|e| anyhow!("{e}"))?;
    Ok(win)
}

/// Cierra el HUD (y con él la cámara).
pub fn hide_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(OVERLAY) {
        let _ = w.close();
    }
}

/// Cuando la cámara ya está lista, el HUD deja pasar los clics a lo que hay debajo.
pub fn set_click_through(app: &AppHandle, on: bool) {
    if let Some(w) = app.get_webview_window(OVERLAY) {
        let _ = w.set_ignore_cursor_events(on);
    }
}

pub fn window_action(action: WindowAction) -> Result<()> {
    window_manager::native().apply(action)
}

pub fn switch_desktop(dir: DesktopDir) -> Result<()> {
    window_manager::native().switch_desktop(dir)
}
