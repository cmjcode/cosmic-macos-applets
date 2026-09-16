// SPDX-License-Identifier: GPL-3.0-only
//! `cosmic-macos-setup`: apply or restore the macOS-style COSMIC top bar.

mod backup;
mod panel_profile;

use anyhow::{Context, Result, bail};
use cosmic_config::{Config, ConfigGet};
use panel_profile::{Change, Options};
use std::{path::PathBuf, process::ExitCode};

const USAGE: &str = "\
cosmic-macos-setup — macOS-style top bar for COSMIC

USAGE:
    cosmic-macos-setup apply [--dry-run] [--force] [--opacity <0.0-1.0>] [--no-weekday] [--keep-notifications]
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

OPTIONS:
    --dry-run      Print the changes without writing anything.
    --force        Apply even if the macOS applets are not installed yet.
    --opacity N    Panel opacity (default 0.8).
    --no-weekday   Do not show the weekday in the clock.
    --keep-notifications
                   Keep COSMIC's notification applet next to the Control Center.
";

#[derive(Debug, PartialEq)]
enum RestoreTarget {
    Latest,
    First,
    Dir(PathBuf),
}

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
    Help,
}

impl PartialEq for Options {
    fn eq(&self, other: &Self) -> bool {
        (self.opacity - other.opacity).abs() < f32::EPSILON
            && self.clock_weekday == other.clock_weekday
    }
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
        other => bail!("unknown command: {other}\n\n{USAGE}"),
    }
}

fn open(component: &str) -> Result<Config> {
    Config::new(component, 1).with_context(|| format!("open config {component}"))
}

fn print_changes(changes: &[Change]) {
    for change in changes {
        println!(
            "  {}/{}\n      {} -> {}",
            change.component, change.key, change.current, change.desired
        );
    }
}

fn collect_changes(options: &Options) -> Result<(Config, Config, Vec<Change>)> {
    let entries: Vec<String> = open(panel_profile::PANEL_LIST_COMPONENT)?
        .get("entries")
        .unwrap_or_default();
    if !entries.iter().any(|e| e == "Panel") {
        bail!(
            "no COSMIC panel named \"Panel\" is configured (found: {entries:?}).\n\
             Enable the top panel in Settings › Desktop › Panel first."
        );
    }

    let panel = open(panel_profile::PANEL_COMPONENT)?;
    let time = open(panel_profile::TIME_COMPONENT)?;
    let right = panel_profile::right_applets(options, panel_profile::applet_installed);
    let mut changes = panel_profile::panel_changes(&panel, options, right);
    changes.extend(panel_profile::time_changes(&time, options));
    Ok((panel, time, changes))
}

fn apply(dry_run: bool, force: bool, options: &Options) -> Result<()> {
    let missing: Vec<&str> = panel_profile::OWN_APPLETS
        .iter()
        .copied()
        .filter(|id| !panel_profile::applet_installed(id))
        .collect();
    if !missing.is_empty() && !force {
        bail!(
            "these applets are not installed: {missing:?}\n\
             Run `just install` first, or pass --force."
        );
    }

    let (panel, time, changes) = collect_changes(options)?;
    if changes.is_empty() {
        println!("The macOS profile is already applied. Nothing to do.");
        return Ok(());
    }
    println!("{} setting(s) will change:", changes.len());
    print_changes(&changes);
    if dry_run {
        println!("\nDry run: nothing was written.");
        return Ok(());
    }

    let backup_dir = backup::create(&backup::cosmic_config_dir()?, &backup::backups_dir()?)
        .context("backup failed; nothing was changed")?;
    println!("\nBackup saved to {}", backup_dir.display());

    for change in &changes {
        let config = if change.component == panel_profile::TIME_COMPONENT {
            &time
        } else {
            &panel
        };
        if let Err(error) = change.apply(config) {
            eprintln!("error: {error:#}\nRolling back…");
            backup::restore(&backup::cosmic_config_dir()?, &backup_dir)
                .context("rollback failed; restore manually with `cosmic-macos-setup restore`")?;
            bail!("apply failed and the previous configuration was restored");
        }
    }
    println!(
        "Applied. Undo with `cosmic-macos-setup restore` (or `restore --first` for the original panel)"
    );
    Ok(())
}

fn restore(target: RestoreTarget) -> Result<()> {
    let dir = match target {
        RestoreTarget::Dir(dir) => dir,
        RestoreTarget::Latest => backup::list(&backup::backups_dir()?)?
            .pop()
            .context("no backups found")?,
        RestoreTarget::First => backup::list(&backup::backups_dir()?)?
            .into_iter()
            .next()
            .context("no backups found")?,
    };
    let changes = backup::restore(&backup::cosmic_config_dir()?, &dir)?;
    println!("Restored {} ({changes} key(s) changed).", dir.display());
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
            let (_, _, changes) = collect_changes(&Options::default())?;
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
            "apply --dry-run --opacity 0.5 --no-weekday --keep-notifications",
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
                },
            }
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
