// SPDX-License-Identifier: GPL-3.0-only
//! Pure logic choosing which menu belongs to the focused window.

use std::path::{Path, PathBuf};

use super::{procinfo::ProcInfo, x11::ActiveX11Window};

/// The focused toplevel as the panel sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    pub app_id: String,
    /// Program from the app's desktop entry `Exec=`, if known.
    pub program: Option<String>,
    /// `program` resolved through `$PATH` and symlinks (filled in by the service).
    pub program_path: Option<PathBuf>,
    /// Position of the focused window among the app's windows, oldest first,
    /// and how many the app has. Used to pick one of several menu bars.
    pub window_index: usize,
    pub window_count: usize,
}

/// A registration together with the identity of its exporting process.
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    pub window_id: u32,
    pub service: &'a str,
    pub path: &'a str,
    pub seq: u64,
    pub proc: Option<&'a ProcInfo>,
}

fn normalize(s: &str) -> String {
    s.trim().trim_end_matches(".desktop").to_lowercase()
}

/// Names the focused app may appear under: its app id, the last segment of a
/// reverse-DNS id (`org.kde.kate` → `kate`), and its desktop entry program.
#[must_use]
pub fn focus_names(focus: &Focus) -> Vec<String> {
    let id = normalize(&focus.app_id);
    let mut names = Vec::new();
    if !id.is_empty() {
        if let Some(last) = id.rsplit('.').next().filter(|l| *l != id) {
            names.push(last.to_owned());
        }
        names.push(id);
    }
    if let Some(program) = focus
        .program
        .as_deref()
        .map(normalize)
        .filter(|p| !p.is_empty())
    {
        names.push(program);
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// `true` if `exe` is `program` itself, or the real binary a launcher script
/// sits next to: `…/program/soffice` → `…/program/soffice.bin`,
/// `…/firefox/firefox` → `…/firefox/firefox-bin`.
#[must_use]
pub fn launcher_matches(program: &Path, exe: &Path) -> bool {
    if program == exe {
        return true;
    }
    let (Some(pdir), Some(edir)) = (program.parent(), exe.parent()) else {
        return false;
    };
    let (Some(pname), Some(ename)) = (
        program.file_name().and_then(|n| n.to_str()),
        exe.file_name().and_then(|n| n.to_str()),
    ) else {
        return false;
    };
    pdir == edir
        && pname.len() >= 3
        && ename
            .strip_prefix(pname)
            .is_some_and(|rest| rest.starts_with(['.', '-', '_']))
}

fn names_match(proc: &ProcInfo, names: &[String]) -> bool {
    if proc.names().iter().any(|n| names.iter().any(|m| m == n)) {
        return true;
    }
    // `comm` is cut to 15 bytes: accept it as a prefix only when truncated.
    proc.comm.as_deref().is_some_and(|comm| {
        names
            .iter()
            .any(|m| m == comm || (comm.len() == 15 && m.starts_with(comm)))
    })
}

/// Does `proc` look like the focused application?
#[must_use]
pub fn identity_matches(proc: &ProcInfo, focus: &Focus) -> bool {
    names_match(proc, &focus_names(focus))
        || matches!(
            (focus.program_path.as_deref(), proc.exe_path.as_deref()),
            (Some(program), Some(exe)) if launcher_matches(program, exe)
        )
}

/// Pick the registered menu for the focused window.
///
/// 1. An X11 window has focus: use the registration for its window id, if its
///    `WM_CLASS` agrees with the focused app (guards against stale X focus).
/// 2. Otherwise match the exporting process against the app's identity and
///    take that process's newest registration.
#[must_use]
pub fn select<'a>(
    focus: &Focus,
    active_x11: Option<&ActiveX11Window>,
    candidates: &'a [Candidate<'a>],
) -> Option<&'a Candidate<'a>> {
    if focus.app_id.is_empty() {
        return None;
    }
    let names = focus_names(focus);

    if let Some(x11) = active_x11 {
        let class_agrees = x11.class.is_empty() || x11.class.iter().any(|c| names.contains(c));
        if class_agrees
            && let Some(hit) = candidates
                .iter()
                .filter(|c| c.window_id == x11.id)
                .max_by_key(|c| c.seq)
        {
            return Some(hit);
        }
    }

    candidates
        .iter()
        .filter(|c| c.proc.is_some_and(|p| identity_matches(p, focus)))
        .max_by_key(|c| c.seq)
}

/// Numeric children of a `/MenuBar` introspection document, sorted.
/// Qt exports one `/MenuBar/<n>` object per window with a menu bar.
#[must_use]
pub fn menubar_children(introspection_xml: &str) -> Vec<u32> {
    let mut ids: Vec<u32> = introspection_xml
        .split("<node name=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next()?.parse().ok())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Choose the menu bar of the focused window among an app's menu bars.
///
/// Qt numbers menu bars in window creation order, and the compositor's window
/// ids grow the same way, so when both counts agree the focused window's
/// position picks its menu bar. Otherwise take the newest one.
#[must_use]
pub fn pick_menubar(menubars: &[u32], window_index: usize, window_count: usize) -> Option<u32> {
    if menubars.len() == window_count {
        menubars.get(window_index).copied()
    } else {
        menubars.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(exe: &str) -> ProcInfo {
        ProcInfo {
            pid: 1,
            exe_path: Some(PathBuf::from(format!("/usr/bin/{exe}"))),
            exe: Some(exe.into()),
            argv0: Some(exe.into()),
            comm: Some(exe.chars().take(15).collect()),
            flatpak_id: None,
        }
    }

    fn cand<'a>(window_id: u32, service: &'a str, seq: u64, p: &'a ProcInfo) -> Candidate<'a> {
        Candidate {
            window_id,
            service,
            path: "/MenuBar/1",
            seq,
            proc: Some(p),
        }
    }

    fn focus(app_id: &str) -> Focus {
        Focus {
            app_id: app_id.into(),
            ..Focus::default()
        }
    }

    #[test]
    fn wayland_app_matches_by_process_identity() {
        let kate = proc("kate");
        let qv = proc("qv4l2");
        let cands = [cand(1, ":1.5", 1, &kate), cand(1, ":1.9", 2, &qv)];
        assert_eq!(
            select(&focus("org.kde.kate"), None, &cands)
                .unwrap()
                .service,
            ":1.5"
        );
        assert_eq!(
            select(&focus("qv4l2"), None, &cands).unwrap().service,
            ":1.9"
        );
        assert!(select(&focus("firefox"), None, &cands).is_none());
    }

    #[test]
    fn newest_registration_of_a_process_wins() {
        let kate = proc("kate");
        let cands = [cand(1, ":1.5", 1, &kate), cand(2, ":1.5", 7, &kate)];
        assert_eq!(select(&focus("kate"), None, &cands).unwrap().window_id, 2);
    }

    #[test]
    fn desktop_program_bridges_different_app_ids() {
        let code = proc("code-oss");
        let cands = [cand(3, ":1.2", 1, &code)];
        let f = Focus {
            app_id: "code-url-handler".into(),
            program: Some("code-oss".into()),
            ..Focus::default()
        };
        assert!(select(&f, None, &cands).is_some());
        assert!(select(&focus("code-url-handler"), None, &cands).is_none());
    }

    #[test]
    fn truncated_comm_matches_as_prefix_only_when_truncated() {
        let mut p = proc("x");
        p.exe = Some("java".into());
        p.exe_path = None;
        p.argv0 = Some("java".into());
        p.comm = Some("jetbrains-toolb".into());
        let cands = [cand(1, ":1.1", 1, &p)];
        assert!(select(&focus("jetbrains-toolbox"), None, &cands).is_some());
        p.comm = Some("kat".into());
        let cands = [cand(1, ":1.1", 1, &p)];
        assert!(select(&focus("kate"), None, &cands).is_none());
    }

    #[test]
    fn x11_window_id_wins_when_class_agrees() {
        let java = proc("java");
        let cands = [
            cand(0x0240_0001, ":1.7", 1, &java),
            cand(0x0260_0001, ":1.7", 2, &java),
        ];
        let x11 = ActiveX11Window {
            id: 0x0240_0001,
            class: vec!["jetbrains-idea".into()],
        };
        assert_eq!(
            select(&focus("jetbrains-idea"), Some(&x11), &cands)
                .unwrap()
                .window_id,
            0x0240_0001,
            "exact X11 window beats the newer registration"
        );
        // Stale X focus from another app must not leak its menu.
        assert!(select(&focus("org.kde.kate"), Some(&x11), &cands).is_none());
    }

    #[test]
    fn launcher_scripts_match_their_real_binary() {
        let lo = Path::new("/usr/lib/libreoffice/program/soffice");
        assert!(launcher_matches(
            lo,
            Path::new("/usr/lib/libreoffice/program/soffice.bin")
        ));
        assert!(launcher_matches(
            Path::new("/usr/lib/firefox/firefox"),
            Path::new("/usr/lib/firefox/firefox-bin")
        ));
        assert!(
            !launcher_matches(lo, Path::new("/usr/bin/soffice.bin")),
            "different directory"
        );
        assert!(
            !launcher_matches(Path::new("/usr/bin/go"), Path::new("/usr/bin/gopls")),
            "no separator"
        );
        assert!(
            !launcher_matches(Path::new("/usr/bin/a"), Path::new("/usr/bin/a.bin")),
            "too short"
        );

        let mut soffice = proc("soffice.bin");
        soffice.exe_path = Some("/usr/lib/libreoffice/program/soffice.bin".into());
        let writer = Focus {
            app_id: "libreoffice-writer".into(),
            program: Some("libreoffice".into()),
            program_path: Some(lo.into()),
            ..Focus::default()
        };
        assert!(identity_matches(&soffice, &writer));
        assert!(!identity_matches(
            &soffice,
            &Focus {
                program_path: None,
                ..writer
            }
        ));
    }

    #[test]
    fn parses_menubar_children() {
        let xml = r#"<node><interface name="org.freedesktop.DBus.Introspectable"/>
            <node name="2"/><node name="1"/><node name="notes"/><node name="10"/></node>"#;
        assert_eq!(menubar_children(xml), [1, 2, 10]);
        assert!(menubar_children("<node/>").is_empty());
    }

    #[test]
    fn menubar_follows_window_order_when_counts_agree() {
        assert_eq!(pick_menubar(&[1, 2], 0, 2), Some(1));
        assert_eq!(pick_menubar(&[1, 2], 1, 2), Some(2));
        assert_eq!(
            pick_menubar(&[1, 2], 0, 3),
            Some(2),
            "mismatch falls back to newest"
        );
        assert_eq!(pick_menubar(&[], 0, 0), None);
    }

    #[test]
    fn no_focus_no_menu() {
        let p = proc("kate");
        let cands = [cand(1, ":1.1", 1, &p)];
        assert!(select(&focus(""), None, &cands).is_none());
    }
}
