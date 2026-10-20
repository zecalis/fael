//! Decision 01M5528X: at most 3 live decisions per file at `add`; `--force`
//! escapes; a dormant row (its files changed unjudged) does not count.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(d: &Path, args: &[&str]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join(format!("fael-test-state-{}", std::process::id())),
        )
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    (o.status.success(), text)
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-cap-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for a in [
        &["init", "-q"][..],
        &["config", "user.name", "Cap Test"],
        &["config", "user.email", "cap@example.com"],
    ] {
        Command::new("git")
            .args(a)
            .current_dir(&d)
            .status()
            .unwrap();
    }
    std::fs::write(d.join("src/a.rs"), "v1\n").unwrap();
    d
}

fn add(d: &Path, text: &str, extra: &[&str]) -> (bool, String) {
    let mut a = vec!["add", "decision", text, "--files", "src/a.rs", "--key"];
    let key = format!("cap:{}", text.replace(' ', "-"));
    a.push(&key);
    a.extend(extra);
    fael(d, &a)
}

#[test]
fn fourth_live_decision_is_rejected_with_ready_commands() {
    let d = repo();
    for t in ["one one", "two two", "three three"] {
        let (ok, out) = add(&d, t, &[]);
        assert!(ok, "{out}");
    }
    let (ok, out) = add(&d, "four four", &[]);
    assert!(
        !ok && out.contains("3 live decisions") && out.contains("--supersedes"),
        "{out}"
    );
    assert!(out.contains("fael close"), "{out}");
    assert!(add(&d, "four four", &["--force"]).0);
}

#[test]
fn dormant_rows_do_not_count() {
    let d = repo();
    for t in ["one one", "two two", "three three"] {
        assert!(add(&d, t, &[]).0);
    }
    // every row's only file changed unjudged: all dormant
    std::fs::write(d.join("src/a.rs"), "v2\n").unwrap();
    let (ok, out) = add(&d, "four four", &[]);
    assert!(ok, "{out}");
}

#[test]
fn a_rename_counts_both_names_together() {
    let d = repo();
    std::fs::write(d.join("src/b.rs"), "v1\n").unwrap();
    for t in ["one one", "two two", "three three"] {
        assert!(add(&d, t, &[]).0);
    }
    // the same file under its new path: the 3 rows filed before the move count
    let (mv_ok, mv_out) = fael(&d, &["mv", "src/a.rs", "src/b.rs"]);
    assert!(mv_ok, "{mv_out}");
    let (ok, out) = fael(
        &d,
        &[
            "add",
            "decision",
            "moved over",
            "--files",
            "src/b.rs",
            "--key",
            "cap:moved-over",
        ],
    );
    assert!(!ok && out.contains("3 live decisions"), "{out}");
}

/// The cap counts decisions and a retired row is a note, so the reject needs no
/// merge proof: with a branch-tied note open, a rejected add spawns none of the
/// git that proves a branch merged (fael:01M5CPNJ).
#[cfg(unix)]
#[test]
fn a_rejected_add_runs_no_merge_proof() {
    use std::os::unix::fs::PermissionsExt;
    let d = repo();
    let git = |a: &[&str]| {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(a)
            .current_dir(&d)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {a:?}");
    };
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    git(&["switch", "-qc", "feat/x"]);
    assert!(fael(&d, &["add", "note", "stopped here", "--files", "src/a.rs"]).0);
    git(&["switch", "-q", "-"]);
    for t in ["one one", "two two", "three three"] {
        assert!(add(&d, t, &[]).0);
    }
    let shim = d.join(".shim");
    std::fs::create_dir_all(&shim).unwrap();
    let real = Command::new("which").arg("git").output().unwrap().stdout;
    let real = String::from_utf8(real).unwrap();
    let spawned = shim.join("spawned");
    let script = shim.join("git");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$1\" >> '{}'\nexec '{}' \"$@\"\n",
            spawned.display(),
            real.trim()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", shim.display(), std::env::var("PATH").unwrap());
    let out = Command::new(env!("CARGO_BIN_EXE_fael"))
        .env("PATH", path)
        .env("FAEL_STATE_DIR", d.join(".state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .args(["add", "decision", "four four", "--files", "src/a.rs"])
        .args(["--key", "cap:four"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(!out.status.success(), "rejected");
    let spawned = std::fs::read_to_string(&spawned).unwrap_or_default();
    for proof in ["merge-base", "diff", "log", "rev-list", "for-each-ref"] {
        assert!(!spawned.lines().any(|l| l == proof), "{proof}:\n{spawned}");
    }
}
