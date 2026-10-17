//! `fael chunk show` with the forms agents type: `p/label`, `p:label`, `p label`, `--uid`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> (PathBuf, String) {
    let d = std::env::temp_dir().join(format!("fael-show-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    assert!(fael(&d, &["plan", "new", "pl", "PL"]).0);
    let (ok, uid, err) = fael(
        &d,
        &[
            "chunk", "add", "Post", "--plan", "pl", "--label", "b1", "--brief", "x",
        ],
    );
    assert!(ok, "{err}");
    (d, uid.trim().to_string())
}

#[test]
fn every_natural_form_shows_the_chunk() {
    let (d, uid) = repo();
    for args in [
        vec!["chunk", "show", "pl/b1"],
        vec!["chunk", "show", "pl:b1"],
        vec!["chunk", "show", "pl", "b1"],
        vec!["chunk", "show", "--uid", &uid],
        vec!["chunk", "show", "--plan", "pl", "b1"],
        vec!["chunk", "show", "b1"],
    ] {
        let (ok, out, err) = fael(&d, &args);
        assert!(ok && out.contains(&uid), "{args:?}: {out}{err}");
    }
}

#[test]
fn a_wrong_form_names_the_retype() {
    let (d, _) = repo();
    for args in [
        vec!["chunk", "show", "pl/zz"],
        vec!["chunk", "show", "pl", "zz"],
        vec!["chunk", "show", "nope:b1"],
    ] {
        let (ok, _, err) = fael(&d, &args);
        assert!(
            !ok && err.contains("retype `fael chunk show --plan"),
            "{args:?}: {err}"
        );
    }
}
