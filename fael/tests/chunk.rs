//! `fael chunk …` / `fael run end` through the real binary: the launcher's shell line
//! (`b=$(fael chunk start <uid> --run R) && { claude "$b"; fael run end R; }`) and `--out`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-chunk-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    d
}

fn add(d: &Path) -> String {
    let (ok, out, err) = fael(d, &["chunk", "add", "Post", "--brief", "Write it"]);
    assert!(ok, "{err}");
    out.trim().to_string()
}

#[test]
fn a_lost_start_prints_no_brief_and_its_run_end_ends_nothing() {
    let d = repo();
    let u = add(&d);
    let (ok, brief, _) = fael(
        &d,
        &["chunk", "start", &u, "--run", "R1", "--client", "claude"],
    );
    assert!(
        ok && brief.starts_with("fael run R1 · 1 chunk\n"),
        "{brief}"
    );
    let (ok, out, err) = fael(&d, &["chunk", "start", &u, "--run", "R2"]);
    assert!(!ok && out.is_empty(), "the shell opens no agent: {out}");
    assert!(err.contains("held by run R1 (claude)"), "{err}");
    // the loser's cleanup: exit 0, R1 still holds
    assert!(fael(&d, &["run", "end", "R2"]).0);
    let (_, _, err) = fael(&d, &["chunk", "start", &u, "--run", "R3"]);
    assert!(err.contains("held by run R1"), "{err}");
    assert!(fael(&d, &["run", "end", "R1"]).0);
    assert!(fael(&d, &["run", "end", "R1"]).0, "none left is fine");
    let (ok, _, err) = fael(&d, &["chunk", "wait", &u, "Which tone?"]);
    assert!(!ok && err.contains("--on owner|data"), "{err}");
    let (ok, out, err) = fael(&d, &["chunk", "wait", &u, "Which tone?", "--on", "owner"]);
    assert!(ok, "{err}");
    assert_eq!(out, format!("chunk {u} → waiting on owner\n"));
    let (_, out, _) = fael(&d, &["chunk", "answer", &u, "Warm"]);
    assert_eq!(out, format!("chunk {u} → open\n"));
    let (_, brief, _) = fael(&d, &["chunk", "start", &u]);
    assert!(brief.contains("owner said:\n> Warm\n"), "{brief}");
    assert!(!fael(&d, &["chunk", "bogus", &u]).0);
}

#[test]
fn done_out_copies_the_output_per_run() {
    let d = repo();
    let u = add(&d);
    assert!(fael(&d, &["chunk", "start", &u, "--run", "R1"]).0);
    let (ok, _, err) = fael(&d, &["chunk", "done", &u, "h", "--out", "nope.png"]);
    assert!(!ok && err.contains("does not exist"), "{err}");
    std::fs::create_dir_all(d.join("out/img")).unwrap();
    std::fs::write(d.join("out/img/a.png"), "png").unwrap();
    let (ok, _, err) = fael(&d, &["chunk", "done", &u, "two images", "--out", "out/img"]);
    assert!(ok, "{err}");
    let kept = d.join(".git/fael/out").join(&u).join("R1/img/a.png");
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "png");
    let (_, out, _) = fael(&d, &["plan", "export", "inbox"]);
    assert!(out.contains(" · review"), "{out}");
    let (_, out, _) = fael(&d, &["chunk", "accept", &u]);
    assert_eq!(out, format!("chunk {u} → done\n"));
}

/// `fael hook stop` with this worktree as cwd: the event reaches plans.db.
fn stop(d: &Path, session: &str) {
    use std::io::Write as _;
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["hook", "stop"])
        .current_dir(d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("FAEL_DIR")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let e = serde_json::json!({"cwd": d, "session": session, "text": ""});
    c.stdin
        .take()
        .unwrap()
        .write_all(e.to_string().as_bytes())
        .unwrap();
    assert!(c.wait().unwrap().success());
}

#[test]
fn the_stop_hook_stamps_its_worktree_run() {
    let d = repo();
    let u = add(&d);
    assert!(fael(&d, &["chunk", "start", &u, "--run", "R1"]).0);
    stop(&d, "2026-10-11T09:00:00Z");
    // the hook filled `session`: another session's stamp now finds no run
    let wt = d.canonicalize().unwrap().to_string_lossy().into_owned();
    let mut s = fael_core::plan::Store::open(&d.join(".git/fael/plans.db")).unwrap();
    assert_eq!(s.seen(&wt, "2026-10-11T10:00:00Z", "t").unwrap(), 0);
    assert_eq!(s.seen(&wt, "2026-10-11T09:00:00Z", "t").unwrap(), 1);
}
