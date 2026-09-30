//! `fael doctor` notices client wiring that is behind the binary (a hook that
//! shipped after `fael install` last ran), and stays silent once it is current.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(home: &Path, cwd: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("FAEL_STATE_DIR", home.join("state"))
        // fael on PATH (install refuses without it), no claude CLI
        .env(
            "PATH",
            std::env::join_paths([
                Path::new(env!("CARGO_BIN_EXE_fael")).parent().unwrap(),
                Path::new("/usr/bin"),
                Path::new("/bin"),
            ])
            .unwrap(),
        )
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn scratch() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("fael-wiring-{}", fael_core::ulid()));
    let (home, repo) = (base.join("home"), base.join("repo"));
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );
    (home, repo)
}

#[test]
fn doctor_reports_wiring_behind_the_binary_and_clears_after_install() {
    let (home, repo) = scratch();
    let out = fael(&home, &repo, &["doctor"]);
    assert!(out.contains("[Wiring]"), "nothing installed yet: {out}");

    fael(&home, &repo, &["install", "--client", "claude"]);
    let out = fael(&home, &repo, &["doctor"]);
    assert!(!out.contains("[Wiring]"), "wiring is current: {out}");

    // an install from before SubagentStop shipped
    let settings = home.join(".claude/settings.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    v["hooks"].as_object_mut().unwrap().remove("SubagentStop");
    std::fs::write(&settings, v.to_string()).unwrap();
    let out = fael(&home, &repo, &["doctor"]);
    assert!(out.contains("[Wiring]"), "stale hooks: {out}");
    assert!(out.contains("fael upgrade"), "{out}");
}

/// session-start output for `repo`, with `home` as the machine's home.
fn session_start(home: &Path, repo: &Path) -> String {
    use std::io::Write;
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["hook", "session-start", "--client", "claude"])
        .current_dir(repo)
        .env("HOME", home)
        .env("FAEL_STATE_DIR", home.join("state"))
        // the same fael on PATH as `install` saw, or every entry reads as a repoint
        .env(
            "PATH",
            Path::new(env!("CARGO_BIN_EXE_fael")).parent().unwrap(),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = format!(r#"{{"cwd":"{}","session_id":"s1"}}"#, repo.display());
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn session_start_says_the_wiring_is_behind_until_install() {
    let (home, repo) = scratch();
    std::fs::write(repo.join("a.rs"), "").unwrap();
    fael(&home, &repo, &["add", "note", "adopted", "--files", "a.rs"]);
    let out = session_start(&home, &repo);
    assert!(
        out.contains("client wiring change(s) behind"),
        "nothing installed yet: {out}"
    );
    assert!(out.contains("fael upgrade"), "{out}");

    fael(&home, &repo, &["install", "--client", "claude"]);
    let out = session_start(&home, &repo);
    assert!(!out.contains("wiring"), "wiring is current: {out}");
}
