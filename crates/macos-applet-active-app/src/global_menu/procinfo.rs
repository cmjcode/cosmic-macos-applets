// SPDX-License-Identifier: GPL-3.0-only
//! Names identifying a process, read from `/proc`.

use std::path::{Path, PathBuf};

/// Lowercased names a process is known by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    /// Full executable path (`/proc/<pid>/exe`).
    pub exe_path: Option<PathBuf>,
    /// Executable file name (`/proc/<pid>/exe`).
    pub exe: Option<String>,
    /// File name of `argv[0]`.
    pub argv0: Option<String>,
    /// Kernel command name; truncated to 15 bytes by Linux.
    pub comm: Option<String>,
    /// Flatpak application id, if sandboxed.
    pub flatpak_id: Option<String>,
}

fn file_name(s: &str) -> Option<String> {
    let name = s.rsplit('/').next()?.trim();
    (!name.is_empty()).then(|| name.to_lowercase())
}

impl ProcInfo {
    /// Read identity names for `pid` under `proc_root` (normally `/proc`).
    #[must_use]
    pub fn read(proc_root: &Path, pid: u32) -> Self {
        let dir = proc_root.join(pid.to_string());
        let exe_path = std::fs::read_link(dir.join("exe"))
            .ok()
            .and_then(|p| Some(PathBuf::from(p.to_str()?.trim_end_matches(" (deleted)"))));
        let exe = exe_path.as_deref().and_then(|p| file_name(p.to_str()?));
        let argv0 = std::fs::read(dir.join("cmdline")).ok().and_then(|bytes| {
            let first = bytes.split(|b| *b == 0).next()?;
            file_name(std::str::from_utf8(first).ok()?)
        });
        let comm = std::fs::read_to_string(dir.join("comm"))
            .ok()
            .and_then(|c| file_name(c.trim()));
        let flatpak_id = std::fs::read_to_string(dir.join("root/.flatpak-info"))
            .ok()
            .and_then(|info| {
                info.lines()
                    .find_map(|l| l.strip_prefix("name="))
                    .map(|n| n.trim().to_lowercase())
            });
        Self {
            pid,
            exe_path,
            exe,
            argv0,
            comm,
            flatpak_id,
        }
    }

    /// All distinct names, for matching.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = [&self.exe, &self.argv0, &self.flatpak_id]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        names.dedup();
        names
    }
}

/// Resolve a desktop entry program (`libreoffice`, `/opt/app/bin/app`) to its
/// canonical file, following `$PATH` and symlinks.
#[must_use]
pub fn resolve_program(program: &str, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    if program.contains('/') {
        return std::fs::canonicalize(program).ok();
    }
    std::env::split_paths(path_var?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
        .and_then(|found| std::fs::canonicalize(found).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_process() {
        let me = ProcInfo::read(Path::new("/proc"), std::process::id());
        assert!(me.exe.is_some(), "{me:?}");
        assert!(me.comm.is_some());
        assert!(me.names().iter().all(|n| *n == n.to_lowercase()));
    }

    #[test]
    fn resolves_programs_through_path_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let real_dir = tmp.path().join("lib/office/program");
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&real_dir).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(real_dir.join("soffice"), "#!/bin/sh").unwrap();
        std::os::unix::fs::symlink(real_dir.join("soffice"), bin.join("libreoffice")).unwrap();

        let path_var = std::env::join_paths([tmp.path().join("nope"), bin.clone()]).unwrap();
        let resolved = resolve_program("libreoffice", Some(&path_var)).unwrap();
        assert_eq!(
            resolved,
            std::fs::canonicalize(real_dir.join("soffice")).unwrap()
        );
        assert!(resolve_program("missing", Some(&path_var)).is_none());
        assert!(resolve_program("", Some(&path_var)).is_none());
        assert_eq!(
            resolve_program(bin.join("libreoffice").to_str().unwrap(), None),
            Some(resolved)
        );
    }

    #[test]
    fn missing_process_yields_empty_info() {
        let tmp = tempfile::tempdir().unwrap();
        let info = ProcInfo::read(tmp.path(), 42);
        assert_eq!(
            info,
            ProcInfo {
                pid: 42,
                ..ProcInfo::default()
            }
        );
    }

    #[test]
    fn parses_fake_proc_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("7");
        std::fs::create_dir_all(dir.join("root")).unwrap();
        std::fs::write(dir.join("cmdline"), b"/usr/bin/Kate\0--new\0").unwrap();
        std::fs::write(dir.join("comm"), "kate\n").unwrap();
        std::fs::write(
            dir.join("root/.flatpak-info"),
            "[Application]\nname=org.kde.kate\n",
        )
        .unwrap();
        std::os::unix::fs::symlink("/usr/bin/kate (deleted)", dir.join("exe")).unwrap();
        let info = ProcInfo::read(tmp.path(), 7);
        assert_eq!(info.exe.as_deref(), Some("kate"));
        assert_eq!(info.exe_path.as_deref(), Some(Path::new("/usr/bin/kate")));
        assert_eq!(info.argv0.as_deref(), Some("kate"));
        assert_eq!(info.flatpak_id.as_deref(), Some("org.kde.kate"));
        assert_eq!(info.names(), ["kate", "org.kde.kate"]);
    }
}
