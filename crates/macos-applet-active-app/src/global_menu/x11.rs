// SPDX-License-Identifier: GPL-3.0-only
//! Short-lived XWayland queries for the focused X11 window.
//!
//! No connection is kept open: a persistent X client would keep XWayland
//! alive (cosmic-comp starts it with `-terminate`). Queries only run while
//! XWayland is already running, so they never cause it to start.

use std::{path::Path, time::Duration};

use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt},
    rust_connection::RustConnection,
};

/// The focused X11 window and its `WM_CLASS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveX11Window {
    pub id: u32,
    /// Lowercased `WM_CLASS` instance and class names.
    pub class: Vec<String>,
}

/// Is an `Xwayland` process running? Scans `/proc`, which takes well under a
/// millisecond on a desktop system.
#[must_use]
pub fn xwayland_running(proc_root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
            && std::fs::read_to_string(entry.path().join("comm"))
                .is_ok_and(|comm| comm.trim() == "Xwayland")
    })
}

/// Split a raw `WM_CLASS` property (`instance\0class\0`).
#[must_use]
pub fn parse_wm_class(raw: &[u8]) -> Vec<String> {
    raw.split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_lowercase())
        .collect()
}

fn query() -> Option<ActiveX11Window> {
    let (conn, screen) = RustConnection::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let atom = conn
        .intern_atom(true, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;
    if atom == 0 {
        return None;
    }
    let id = conn
        .get_property(false, root, atom, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()?;
    if id == 0 {
        // A Wayland window has focus.
        return None;
    }
    let class = conn
        .get_property(false, id, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        .ok()?
        .reply()
        .map(|reply| parse_wm_class(&reply.value))
        .unwrap_or_default();
    Some(ActiveX11Window { id, class })
}

/// The focused X11 window, if XWayland is running and an X11 window has focus.
pub async fn active_window() -> Option<ActiveX11Window> {
    if !xwayland_running(Path::new("/proc")) {
        return None;
    }
    let task = tokio::task::spawn_blocking(query);
    tokio::time::timeout(Duration::from_millis(500), task)
        .await
        .ok()?
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_class_is_split_and_lowercased() {
        assert_eq!(
            parse_wm_class(b"jetbrains-idea\0jetbrains-IDEA\0"),
            ["jetbrains-idea", "jetbrains-idea"]
        );
        assert!(parse_wm_class(b"").is_empty());
    }

    #[test]
    fn detects_xwayland_in_fake_proc() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("12")).unwrap();
        std::fs::write(tmp.path().join("12/comm"), "bash\n").unwrap();
        assert!(!xwayland_running(tmp.path()));
        std::fs::create_dir_all(tmp.path().join("99")).unwrap();
        std::fs::write(tmp.path().join("99/comm"), "Xwayland\n").unwrap();
        assert!(xwayland_running(tmp.path()));
    }
}
