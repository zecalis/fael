//! `fael upgrade`'s binary half (PLAN-fael-auto-update chunk 2): which channel
//! installed this binary, and the command that updates it. The wiring half is
//! `install::cmd`, run by the *new* binary once this one has been replaced.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, PartialEq)]
pub(crate) enum Channel {
    Brew,
    Npm,
    /// cargo-dist's shell/ps1 installer (`install-path = "CARGO_HOME"`)
    Installer,
    Unknown,
}

impl Channel {
    fn name(&self) -> &'static str {
        match self {
            Channel::Brew => "brew",
            Channel::Npm => "npm",
            Channel::Installer => "shell/ps1 installer",
            Channel::Unknown => "unknown",
        }
    }
}

/// The channel from where the binary sits. `exe` is canonical, so a brew
/// symlink in `bin/` already points into the Cellar.
pub(super) fn detect(exe: &Path, cargo_home: &Path) -> Channel {
    let p = exe.to_string_lossy().replace('\\', "/");
    if p.contains("/Cellar/") {
        Channel::Brew
    } else if p.contains("/node_modules/@zecalis/fael/") {
        Channel::Npm
    } else if exe.starts_with(cargo_home.join("bin")) {
        Channel::Installer
    } else {
        Channel::Unknown
    }
}

/// The update command of a channel; `None` when there is nothing to run
/// (an unknown channel, or an installer with no `fael-update` beside it —
/// a pre-receipt install: run the installer once more).
pub(super) fn command(ch: &Channel, exe: &Path) -> Option<Vec<String>> {
    let s = |v: &[&str]| Some(v.iter().map(|x| x.to_string()).collect());
    match ch {
        Channel::Brew => s(&["brew", "upgrade", "zecalis/tap/fael"]),
        Channel::Npm => s(&["npm", "i", "-g", "@zecalis/fael"]),
        Channel::Installer => ["fael-update", "fael-update.exe"]
            .into_iter()
            .map(|n| exe.with_file_name(n))
            .find(|p| p.is_file())
            .map(|p| vec![p.to_string_lossy().into_owned()]),
        Channel::Unknown => None,
    }
}

pub(super) fn cargo_home(home: &Path) -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cargo"))
}

/// What `fael upgrade` does before the wiring: update the binary, then hand
/// the wiring to the new one. `Ok(true)` = the new binary did the wiring,
/// nothing is left for this process. `dry` only says what would run.
pub(crate) fn binary(home: &Path, args: &[String], dry: bool, yes: bool) -> Result<bool, String> {
    let exe = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .map_err(|e| format!("fael upgrade: {e}"))?;
    let ch = detect(&exe, &cargo_home(home));
    let Some(cmd) = command(&ch, &exe) else {
        println!(
            "channel: {} — no command to update the binary itself",
            ch.name()
        );
        if ch == Channel::Installer {
            println!("  no fael-update beside it: run the installer once more (it adds one)");
        }
        return Ok(false);
    };
    println!("channel: {} — runs `{}`", ch.name(), cmd.join(" "));
    if dry {
        return Ok(false);
    }
    if !yes && std::io::stdin().is_terminal() {
        print!("update the binary now, then its wiring? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        if !line.trim().eq_ignore_ascii_case("y") {
            return Ok(false);
        }
    }
    let ok = Command::new(&cmd[0]).args(&cmd[1..]).status();
    if !ok.is_ok_and(|s| s.success()) {
        return Err(format!(
            "fael upgrade: `{}` failed — nothing else was changed",
            cmd.join(" ")
        ));
    }
    // the new binary (same path for brew/installer; the PATH one for npm) ships
    // the new hook table and skill, so it writes the wiring: --wiring skips this step
    let new = super::hook_exe().map_or(exe, PathBuf::from);
    let s = Command::new(new)
        .args(["upgrade", "--wiring", "--yes"])
        .args(args)
        .status()
        .map_err(|e| format!("fael upgrade: the new binary did not start: {e}"))?;
    if s.success() {
        Ok(true)
    } else {
        Err("fael upgrade: the new binary's wiring pass failed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_comes_from_where_the_binary_sits() {
        let ch = |p: &str| detect(Path::new(p), Path::new("/h/.cargo"));
        assert_eq!(
            ch("/opt/homebrew/Cellar/fael/0.29.0/bin/fael"),
            Channel::Brew
        );
        assert_eq!(
            ch("/home/linuxbrew/.linuxbrew/Cellar/fael/1/bin/fael"),
            Channel::Brew
        );
        assert_eq!(
            ch("/usr/lib/node_modules/@zecalis/fael/node_modules/.bin_real/fael"),
            Channel::Npm
        );
        assert_eq!(
            ch(r"C:\u\AppData\npm\node_modules\@zecalis\fael\node_modules\.bin_real\fael.exe"),
            Channel::Npm
        );
        assert_eq!(ch("/h/.cargo/bin/fael"), Channel::Installer);
        assert_eq!(ch("/tmp/x/fael"), Channel::Unknown);
    }

    #[test]
    fn commands_per_channel() {
        let e = Path::new("/nowhere/fael");
        assert_eq!(
            command(&Channel::Brew, e).unwrap().join(" "),
            "brew upgrade zecalis/tap/fael"
        );
        assert_eq!(
            command(&Channel::Npm, e).unwrap().join(" "),
            "npm i -g @zecalis/fael"
        );
        assert!(
            command(&Channel::Installer, e).is_none(),
            "no fael-update beside it"
        );
        assert!(command(&Channel::Unknown, e).is_none());
    }
}
