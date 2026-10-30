//! `fael install --preset multi-agent`: write the repo's `.fael/config.toml`
//! from a bundle of proven primitives only (plan fael-secretary c7). No new
//! behavior — the body parses to `Config::default()` — so a fresh clone gets
//! identical semantics with zero hand-tuning. Never overwrites a repo config
//! that already exists; never touches a preset it does not know.

use super::Ctx;
use std::path::PathBuf;

/// Repo root for the preset: the git checkout around the cwd, else the cwd.
fn root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("fael install: cwd: {e}"))?;
    Ok(fael_core::repo_at(&cwd).map(|r| r.root).unwrap_or(cwd))
}

/// Apply the preset in the normal pass (fael is on PATH): writes the missing
/// file and counts it, leaves an existing config alone.
pub(super) fn apply(c: &Ctx, name: &str) -> Result<(), String> {
    let body = fael_core::preset_toml(name)?;
    let path = root()?.join(".fael/config.toml");
    match std::fs::read_to_string(&path) {
        Ok(cur) if cur == body => {}
        Ok(_) => {
            if !c.quiet {
                println!("  preset: config exists, left alone → {}", path.display());
            }
        }
        Err(_) => {
            if !c.dry {
                if let Some(p) = path.parent() {
                    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
                }
                std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            c.say("preset", &path);
        }
    }
    Ok(())
}

/// Apply the preset when fael is not on PATH (no hook wiring possible): the
/// repo config still lands, so wiring can follow once fael is installed.
pub(super) fn apply_without_exe(name: &str, dry: bool) -> Result<(), String> {
    let body = fael_core::preset_toml(name)?;
    let path = root()?.join(".fael/config.toml");
    if std::fs::read_to_string(&path).is_ok() {
        println!("preset: config exists, left alone → {}", path.display());
        return Ok(());
    }
    if dry {
        println!("dry run — nothing is written");
        println!("  would write preset → {}", path.display());
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("  wrote preset → {}", path.display());
    println!("\n1 change(s) applied");
    Ok(())
}
