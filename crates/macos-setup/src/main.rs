// SPDX-License-Identifier: GPL-3.0-only
//! `cosmic-macos-setup`: apply or restore the macOS-style COSMIC top bar.

use anyhow::{Context, Result, bail};
use macos_setup::{
    Change, Options, RestoreTarget, Restored, Service, backup, panel_profile,
    profile::{self, on_off},
    window_controls,
};
use std::{path::PathBuf, process::ExitCode};

const USAGE: &str = "\
cosmic-macos-setup — macOS-style top bar for COSMIC

USAGE:
    cosmic-macos-setup apply [--dry-run] [--force] [--opacity <0.0-1.0>] [--no-weekday] [--keep-notifications]
                             [--global-menu | --no-global-menu]
                             [--window-controls-left | --window-controls-right]
                             [--three-finger-drag | --no-three-finger-drag]
    cosmic-macos-setup restore [--first | <backup-dir>]
    cosmic-macos-setup backups
    cosmic-macos-setup status

COMMANDS:
    apply     Back up the current panel config, then apply the macOS profile.
              The panel updates live; no logout is needed.
    restore   Restore the most recent backup, the first one ever made
              (--first, i.e. the configuration before this tool), or the given one.
    backups   List available backups.
    status    Show which settings differ from the profile.
    window-controls-watch
              Keep window controls on the left (run by the user service that
              --window-controls-left installs).

OPTIONS:
    --dry-run      Print the changes without writing anything.
    --force        Apply even if the macOS applets are not installed yet.
    --opacity N    Panel opacity (default 0.8).
    --no-weekday   Do not show the weekday in the clock.
    --keep-notifications
                   Keep COSMIC's notification applet next to the Control Center.
    --global-menu  Experimental: show app menus (File, Edit, …) in the bar.
                   Restart apps afterwards; only apps exporting their menu over
                   D-Bus take part (Qt apps, X11 apps such as JetBrains IDEs).
                   Without either flag the current setting is kept.
    --no-global-menu
                   Turn the global menu off again.
    --window-controls-left
                   Close, minimize and maximize on the left in GTK, libadwaita
                   and Chromium/Electron apps. Installs a user service that keeps
                   COSMIC from moving them back. COSMIC apps and server-side
                   decorations keep them on the right.
    --window-controls-right
                   Remove that service and put the controls back on the right.
    --three-finger-drag
                   Drag with three fingers on the touchpad. Needs
                   linux-3-finger-drag installed once with sudo; this only
                   switches its user service on.
    --no-three-finger-drag
                   Switch the three-finger drag service off.

Service flags are left unchanged when neither form is given, and `restore`
returns them to their state at backup time.
";

#[derive(Debug, PartialEq)]
enum Command {
    Apply {
        dry_run: bool,
        force: bool,
        options: Options,
    },
    Restore(RestoreTarget),
    Backups,
    Status,
    WindowControlsWatch,
    Help,
}

fn parse(args: &[String]) -> Result<Command> {
    let mut it = args.iter().map(String::as_str);
    let command = match it.next() {
        None | Some("-h" | "--help" | "help") => return Ok(Command::Help),
        Some(c) => c,
    };
    match command {
        "apply" => {
            let (mut dry_run, mut force, mut options) = (false, false, Options::default());
            while let Some(arg) = it.next() {
                match arg {
                    "--dry-run" | "-n" => dry_run = true,
                    "--force" => force = true,
                    "--no-weekday" => options.clock_weekday = false,
                    "--keep-notifications" => options.keep_notifications = true,
                    "--global-menu" => options.global_menu = Some(true),
                    "--no-global-menu" => options.global_menu = Some(false),
                    "--window-controls-left" => options.window_controls_left = Some(true),
                    "--window-controls-right" => options.window_controls_left = Some(false),
                    "--three-finger-drag" => options.three_finger_drag = Some(true),
                    "--no-three-finger-drag" => options.three_finger_drag = Some(false),
                    "--opacity" => {
                        let value = it.next().context("--opacity needs a value")?;
                        let opacity: f32 = value.parse().context("--opacity must be a number")?;
                        if !(0.0..=1.0).contains(&opacity) {
                            bail!("--opacity must be between 0.0 and 1.0");
                        }
                        options.opacity = opacity;
                    }
                    other => bail!("unknown option for apply: {other}"),
                }
            }
            Ok(Command::Apply {
                dry_run,
                force,
                options,
            })
        }
        "restore" => {
            let target = match it.next() {
                None => RestoreTarget::Latest,
                Some("--first") => RestoreTarget::First,
                Some(dir) => RestoreTarget::Dir(PathBuf::from(dir)),
            };
            if let Some(extra) = it.next() {
                bail!("unexpected argument: {extra}");
            }
            Ok(Command::Restore(target))
        }
        "backups" => Ok(Command::Backups),
        "status" => Ok(Command::Status),
        "window-controls-watch" => Ok(Command::WindowControlsWatch),
        other => bail!("unknown command: {other}\n\n{USAGE}"),
    }
}

fn print_changes(changes: &[Change]) {
    for change in changes {
        println!(
            "  {}/{}\n      {} -> {}",
            change.component, change.key, change.current, change.desired
        );
    }
}

fn apply(dry_run: bool, force: bool, options: &Options) -> Result<()> {
    let plan = profile::plan(options, force)?;
    if plan.is_empty() {
        println!("The macOS profile is already applied. Nothing to do.");
        return Ok(());
    }
    println!("{} setting(s) will change:", plan.len());
    print_changes(&plan.changes);
    for (service, want) in &plan.services {
        println!(
            "  service {}\n      {} -> {}",
            service.name(),
            on_off(!want),
            on_off(*want)
        );
    }
    if dry_run {
        println!("\nDry run: nothing was written.");
        return Ok(());
    }
    if let Some(backup_dir) = profile::execute(&plan)? {
        println!("\nBackup saved to {}", backup_dir.display());
    }
    println!(
        "Applied. Undo with `cosmic-macos-setup restore` (or `restore --first` for the original panel)"
    );
    Ok(())
}

fn restore(target: RestoreTarget) -> Result<()> {
    let Restored {
        backup,
        keys,
        services,
    } = profile::restore(target)?;
    for (service, want) in services {
        println!("  {} {}", service.name(), on_off(want));
    }
    println!("Restored {} ({keys} key(s) changed).", backup.display());
    Ok(())
}

fn run(command: Command) -> Result<()> {
    match command {
        Command::Help => print!("{USAGE}"),
        Command::Apply {
            dry_run,
            force,
            options,
        } => apply(dry_run, force, &options)?,
        Command::Restore(dir) => restore(dir)?,
        Command::WindowControlsWatch => window_controls::watch()?,
        Command::Backups => {
            let backups = backup::list(&backup::backups_dir()?)?;
            if backups.is_empty() {
                println!("No backups yet.");
            }
            for dir in backups {
                println!("{}", dir.display());
            }
        }
        Command::Status => {
            for id in panel_profile::OWN_APPLETS {
                let state = if panel_profile::applet_installed(id) {
                    "installed"
                } else {
                    "MISSING"
                };
                println!("applet {id}: {state}");
            }
            for service in Service::ALL {
                println!("service {}: {}", service.name(), on_off(service.enabled()));
            }
            let changes = profile::collect_changes(&profile::current_options())?;
            if changes.is_empty() {
                println!("profile: applied");
            } else {
                println!("profile: {} setting(s) differ", changes.len());
                print_changes(&changes);
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn parses_apply_options() {
        let cmd = parse(&args(
            "apply --dry-run --opacity 0.5 --no-weekday --keep-notifications --global-menu",
        ))
        .unwrap();
        assert_eq!(
            cmd,
            Command::Apply {
                dry_run: true,
                force: false,
                options: Options {
                    opacity: 0.5,
                    clock_weekday: false,
                    keep_notifications: true,
                    global_menu: Some(true),
                    window_controls_left: None,
                    three_finger_drag: None,
                },
            }
        );
    }

    #[test]
    fn global_menu_flag_is_tristate() {
        let menu = |a: &str| match parse(&args(a)).unwrap() {
            Command::Apply { options, .. } => options.global_menu,
            other => panic!("{other:?}"),
        };
        assert_eq!(menu("apply"), None);
        assert_eq!(menu("apply --global-menu"), Some(true));
        assert_eq!(menu("apply --no-global-menu"), Some(false));
    }

    #[test]
    fn service_flags_are_tristate() {
        let opts = |a: &str| match parse(&args(a)).unwrap() {
            Command::Apply { options, .. } => {
                (options.window_controls_left, options.three_finger_drag)
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(opts("apply"), (None, None));
        assert_eq!(
            opts("apply --window-controls-left --three-finger-drag"),
            (Some(true), Some(true))
        );
        assert_eq!(
            opts("apply --window-controls-right --no-three-finger-drag"),
            (Some(false), Some(false))
        );
        assert_eq!(
            parse(&args("window-controls-watch")).unwrap(),
            Command::WindowControlsWatch
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(&args("apply --opacity 2")).is_err());
        assert!(parse(&args("apply --opacity")).is_err());
        assert!(parse(&args("apply --bogus")).is_err());
        assert!(parse(&args("frobnicate")).is_err());
        assert!(parse(&args("restore a b")).is_err());
    }

    #[test]
    fn help_is_default() {
        assert_eq!(parse(&[]).unwrap(), Command::Help);
        assert_eq!(parse(&args("--help")).unwrap(), Command::Help);
        assert_eq!(
            parse(&args("restore /tmp/x")).unwrap(),
            Command::Restore(RestoreTarget::Dir("/tmp/x".into()))
        );
        assert_eq!(
            parse(&args("restore")).unwrap(),
            Command::Restore(RestoreTarget::Latest)
        );
        assert_eq!(
            parse(&args("restore --first")).unwrap(),
            Command::Restore(RestoreTarget::First)
        );
    }
}
