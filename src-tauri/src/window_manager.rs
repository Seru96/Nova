//! Gestor de ventanas multiplataforma para H.A.L.O.
//!
//! Actúa sobre la ventana activa (la que está al frente):
//! - macOS: Accessibility vía System Events (AppleScript) para posición y tamaño,
//!   y atajos del sistema para minimizar y cambiar de escritorio.
//! - Windows: Win32 (`GetForegroundWindow`, `ShowWindow`, `SetWindowPos`).

use anyhow::{anyhow, Result};
use enigo::{Direction, Enigo, Key, Keyboard};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    Maximize,
    Restore,
    Minimize,
    /// Cerrar la ventana activa (no sale de la app).
    Close,
    SnapLeft,
    SnapRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopDir {
    Left,
    Right,
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
/// Rectángulo en coordenadas lógicas (puntos en macOS, píxeles en Windows).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
/// Dónde colocar la ventana para cada acción dentro del área útil de la pantalla.
pub fn target_rect(action: WindowAction, area: Rect) -> Option<Rect> {
    match action {
        WindowAction::Maximize => Some(area),
        WindowAction::SnapLeft => Some(Rect { w: (area.w / 2.0).floor(), ..area }),
        WindowAction::SnapRight => {
            let half = (area.w / 2.0).floor();
            Some(Rect { x: area.x + half, w: area.w - half, ..area })
        }
        WindowAction::Restore => {
            let w = (area.w * 0.62).floor();
            let h = (area.h * 0.7).floor();
            Some(Rect {
                x: area.x + ((area.w - w) / 2.0).floor(),
                y: area.y + ((area.h - h) / 2.0).floor(),
                w,
                h,
            })
        }
        WindowAction::Minimize | WindowAction::Close => None,
    }
}

pub trait WindowManager {
    fn apply(&self, action: WindowAction) -> Result<()>;
    fn switch_desktop(&self, dir: DesktopDir) -> Result<()>;
}

fn enigo() -> Result<Enigo> {
    crate::access::enigo()
}

fn chord(mods: &[Key], key: Key) -> Result<()> {
    let mods = mods.to_vec();
    crate::access::run_keyboard(move || chord_now(&mods, key))
}

fn chord_now(mods: &[Key], key: Key) -> Result<()> {
    let mut e = enigo()?;
    for m in mods {
        e.key(*m, Direction::Press).map_err(|e| anyhow!("{e}"))?;
    }
    let r = e.key(key, Direction::Click);
    for m in mods.iter().rev() {
        let _ = e.key(*m, Direction::Release);
    }
    r.map_err(|e| anyhow!("{e}"))
}

pub fn native() -> impl WindowManager {
    platform::Native
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    pub struct Native;

    /// Área útil de la pantalla principal (sin barra de menús ni Dock), en puntos.
    fn work_area() -> Result<Rect> {
        let out = std::process::Command::new("osascript")
            .args([
                "-l",
                "JavaScript",
                "-e",
                "ObjC.import('AppKit'); var f = $.NSScreen.mainScreen.visibleFrame; var s = $.NSScreen.screens.objectAtIndex(0).frame; \
                 [f.origin.x, s.size.height - f.origin.y - f.size.height, f.size.width, f.size.height].join(',')",
            ])
            .output()?;
        let txt = String::from_utf8_lossy(&out.stdout);
        let v: Vec<f64> = txt.trim().split(',').filter_map(|n| n.trim().parse().ok()).collect();
        if v.len() != 4 {
            return Err(anyhow!("no pude leer el tamaño de la pantalla"));
        }
        Ok(Rect { x: v[0], y: v[1], w: v[2], h: v[3] })
    }

    fn set_frontmost_frame(r: Rect) -> Result<()> {
        let script = format!(
            "tell application \"System Events\"\n\
               set p to first application process whose frontmost is true\n\
               tell window 1 of p\n\
                 set position to {{{x}, {y}}}\n\
                 set size to {{{w}, {h}}}\n\
               end tell\n\
             end tell",
            x = r.x as i64,
            y = r.y as i64,
            w = r.w as i64,
            h = r.h as i64
        );
        let out = std::process::Command::new("osascript").args(["-e", &script]).output()?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow!(
                "macOS no me deja mover la ventana ({}). Revisa el permiso de Accesibilidad de Nova.",
                err.trim()
            ));
        }
        Ok(())
    }

    impl WindowManager for Native {
        fn apply(&self, action: WindowAction) -> Result<()> {
            match action {
                WindowAction::Minimize => chord(&[Key::Meta], Key::Unicode('m')),
                WindowAction::Close => chord(&[Key::Meta], Key::Unicode('w')),
                _ => match target_rect(action, work_area()?) {
                    Some(r) => set_frontmost_frame(r),
                    None => Ok(()),
                },
            }
        }

        fn switch_desktop(&self, dir: DesktopDir) -> Result<()> {
            let key = match dir {
                DesktopDir::Left => Key::LeftArrow,
                DesktopDir::Right => Key::RightArrow,
            };
            chord(&[Key::Control], key)
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, PostMessageW, SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOZORDER, SW_MINIMIZE,
        SW_RESTORE, WM_CLOSE,
    };

    pub struct Native;

    impl WindowManager for Native {
        fn apply(&self, action: WindowAction) -> Result<()> {
            unsafe {
                let hwnd = GetForegroundWindow();
                if hwnd.0.is_null() {
                    return Err(anyhow!("no hay ninguna ventana activa"));
                }
                if action == WindowAction::Minimize {
                    let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    return Ok(());
                }
                if action == WindowAction::Close {
                    PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))?;
                    return Ok(());
                }
                // Sacamos la ventana de maximizada para poder colocarla.
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ = GetMonitorInfoW(mon, &mut info);
                let wa = info.rcWork;
                let area = Rect {
                    x: wa.left as f64,
                    y: wa.top as f64,
                    w: (wa.right - wa.left) as f64,
                    h: (wa.bottom - wa.top) as f64,
                };
                if let Some(r) = target_rect(action, area) {
                    SetWindowPos(
                        hwnd,
                        None,
                        r.x as i32,
                        r.y as i32,
                        r.w as i32,
                        r.h as i32,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )?;
                }
            }
            Ok(())
        }

        fn switch_desktop(&self, dir: DesktopDir) -> Result<()> {
            let key = match dir {
                DesktopDir::Left => Key::LeftArrow,
                DesktopDir::Right => Key::RightArrow,
            };
            chord(&[Key::Control, Key::Meta], key)
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use super::*;

    pub struct Native;

    impl WindowManager for Native {
        fn apply(&self, _action: WindowAction) -> Result<()> {
            Err(anyhow!("gestor de ventanas no disponible en esta plataforma"))
        }
        fn switch_desktop(&self, dir: DesktopDir) -> Result<()> {
            let key = match dir {
                DesktopDir::Left => Key::LeftArrow,
                DesktopDir::Right => Key::RightArrow,
            };
            chord(&[Key::Control, Key::Alt], key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_halves_cover_area() {
        let area = Rect { x: 0.0, y: 25.0, w: 1511.0, h: 900.0 };
        let l = target_rect(WindowAction::SnapLeft, area).unwrap();
        let r = target_rect(WindowAction::SnapRight, area).unwrap();
        assert_eq!(l.x, 0.0);
        assert_eq!(l.w + r.w, area.w);
        assert_eq!(r.x, l.w);
        assert_eq!(l.y, 25.0);
        assert!(target_rect(WindowAction::Minimize, area).is_none());
        assert!(target_rect(WindowAction::Close, area).is_none());
        let c = target_rect(WindowAction::Restore, area).unwrap();
        assert!(c.x > 0.0 && c.x + c.w < area.w);
    }
}
