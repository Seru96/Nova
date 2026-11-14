//! H.A.L.O.: control del ordenador con gestos de la mano.
//!
//! La visión (cámara + 21 puntos por mano + máquina de estados de gestos) vive en la
//! ventana HUD transparente (`overlay`), que nunca se oculta y por eso no la frena el
//! sistema. Este módulo crea esa ventana y ejecuta lo que los gestos piden: mover el
//! ratón, clicar/arrastrar, colocar ventanas y cambiar de escritorio.

use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
#[cfg(not(windows))]
use enigo::Coordinate;
use enigo::{Button, Direction, Mouse};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

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

#[derive(Clone, serde::Serialize)]
struct AccessState {
    ok: bool,
    error: Option<String>,
}

enum Cmd {
    /// Posición normalizada 0..1 sobre el conjunto de pantallas.
    Move(f64, f64),
    Button(ButtonAction),
}

/// Rectángulo que cubre todas las pantallas, en las coordenadas que usa el ratón
/// (puntos en macOS, píxeles físicos en Windows).
#[derive(Debug, Clone, Copy)]
struct Bounds {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn screen_bounds(app: &AppHandle) -> Bounds {
    let mut x0 = f64::MAX;
    let mut y0 = f64::MAX;
    let mut x1 = f64::MIN;
    let mut y1 = f64::MIN;
    for m in app.available_monitors().unwrap_or_default() {
        // En macOS el ratón va en puntos: pasamos de píxeles a puntos con la escala de cada pantalla.
        let k = if cfg!(target_os = "macos") { m.scale_factor() } else { 1.0 };
        let (px, py) = (m.position().x as f64 / k, m.position().y as f64 / k);
        let (w, h) = (m.size().width as f64 / k, m.size().height as f64 / k);
        x0 = x0.min(px);
        y0 = y0.min(py);
        x1 = x1.max(px + w);
        y1 = y1.max(py + h);
    }
    if x0 >= x1 || y0 >= y1 {
        return Bounds { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };
    }
    Bounds { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

#[cfg(windows)]
fn move_cursor(x: i32, y: i32) -> Result<()> {
    // SetCursorPos usa el escritorio virtual completo (todas las pantallas).
    unsafe { windows::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y) }.map_err(|e| anyhow!("{e}"))
}

#[cfg(not(windows))]
fn move_cursor(x: i32, y: i32) -> Result<()> {
    crate::access::with_enigo(move |e| e.move_mouse(x, y, Coordinate::Abs).map_err(|e| anyhow!("{e}")))
}

fn press(b: ButtonAction) -> Result<()> {
    crate::access::with_enigo(move |e| {
        match b {
            ButtonAction::Down => e.button(Button::Left, Direction::Press),
            ButtonAction::Up => e.button(Button::Left, Direction::Release),
            ButtonAction::Click => e.button(Button::Left, Direction::Click),
            ButtonAction::RightClick => e.button(Button::Right, Direction::Click),
        }
        .map_err(|e| anyhow!("{e}"))
    })
}

/// Hilo dedicado al ratón: junta los movimientos atrasados y no bloquea los comandos de Tauri.
pub struct Pointer {
    tx: Sender<Cmd>,
}

impl Pointer {
    pub fn new(app: AppHandle) -> Self {
        let (tx, rx) = channel::<Cmd>();
        std::thread::Builder::new()
            .name("halo-pointer".into())
            .spawn(move || {
                let mut bounds: Option<(Bounds, Instant)> = None;
                // Sin permiso, reintentamos como mucho cada 2 s (y sin repetir el aviso).
                let mut retry_at = Instant::now();
                let mut reported: Option<bool> = None;
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
                    if Instant::now() < retry_at {
                        continue;
                    }
                    let r = match cmd {
                        Cmd::Move(x, y) => {
                            // Las pantallas pueden cambiar (conectar un monitor): refrescamos cada 5 s.
                            let b = match bounds {
                                Some((b, t)) if t.elapsed() < Duration::from_secs(5) => b,
                                _ => {
                                    let b = screen_bounds(&app);
                                    bounds = Some((b, Instant::now()));
                                    b
                                }
                            };
                            let px = (b.x + x.clamp(0.0, 1.0) * (b.w - 1.0)).round() as i32;
                            let py = (b.y + y.clamp(0.0, 1.0) * (b.h - 1.0)).round() as i32;
                            move_cursor(px, py)
                        }
                        Cmd::Button(b) => press(b),
                    };
                    match r {
                        Ok(()) => {
                            if reported != Some(true) {
                                let _ = app.emit("halo://access", AccessState { ok: true, error: None });
                                reported = Some(true);
                            }
                        }
                        Err(e) => {
                            retry_at = Instant::now() + Duration::from_secs(2);
                            if reported != Some(false) {
                                log::warn!("H.A.L.O. sin control del ratón: {e:#}");
                                let _ = app.emit("halo://access", AccessState { ok: false, error: Some(format!("{e:#}")) });
                                reported = Some(false);
                            }
                        }
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

/// Tamaño del HUD: una pastilla pequeña, y más alto si se muestra la cámara.
const HUD_W: f64 = 260.0;
const HUD_H: f64 = 64.0;
const HUD_H_CAMERA: f64 = 230.0;
const HUD_MARGIN: f64 = 16.0;

/// Coloca el HUD en la esquina inferior derecha de la pantalla principal.
pub fn place_overlay(app: &AppHandle, camera: bool) {
    let Some(win) = app.get_webview_window(OVERLAY) else { return };
    let Ok(Some(m)) = win.primary_monitor() else { return };
    let k = m.scale_factor();
    let area = m.work_area();
    let (w, h) = (HUD_W, if camera { HUD_H_CAMERA } else { HUD_H });
    let x = area.position.x as f64 / k + area.size.width as f64 / k - w - HUD_MARGIN;
    let y = area.position.y as f64 / k + area.size.height as f64 / k - h - HUD_MARGIN;
    let _ = win.set_size(LogicalSize::new(w, h));
    let _ = win.set_position(LogicalPosition::new(x, y));
}

/// Muestra (creándola si hace falta) el HUD: una ventana pequeña en una esquina.
/// Antes cubría toda la pantalla y en macOS eso provocaba parpadeos y destellos.
pub fn show_overlay(app: &AppHandle, camera: bool) -> Result<WebviewWindow> {
    let win = match app.get_webview_window(OVERLAY) {
        Some(w) => w,
        None => {
            let w = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("index.html#overlay".into()))
                .title("Nova")
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
    place_overlay(app, camera);
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
