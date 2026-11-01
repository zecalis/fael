//! A follow-up filed with `--left` reads its origin's hand-off in its own brief, with the
//! `measured:` line the origin recorded (issue 01M6BKHZSP).

use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(crate::common::fael_bin())
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

#[test]
fn the_followup_brief_prints_the_origins_handoff_and_its_measure() {
    let d = std::env::temp_dir().join(format!("fael-left-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let add = |t: &str| {
        fael(&d, &["chunk", "add", t, "--brief", "b"])
            .1
            .trim()
            .to_string()
    };
    let (a, b) = (add("Measure"), add("Decide"));
    assert!(fael(&d, &["chunk", "start", &a, "--run", "R1", "--no-worktree"]).0);
    // a measured line without cmd/result is refused, naming what it lacks
    let bad = fael(
        &d,
        &[
            "chunk",
            "done",
            &a,
            "measured: 24/30",
            "--left",
            &b,
            "--no-change",
            "--run",
            "R1",
        ],
    );
    assert!(
        !bad.0 && bad.2.contains("needs cmd: and result:"),
        "{}",
        bad.2
    );
    let h = "ran it\nmeasured: cacheSync hits · cmd: bun bench · result: 24/30";
    let (ok, _, err) = fael(
        &d,
        &[
            "chunk",
            "done",
            &a,
            h,
            "--left",
            &b,
            "--no-change",
            "--run",
            "R1",
        ],
    );
    assert!(ok, "{err}");
    let (ok, brief, err) = fael(&d, &["chunk", "start", &b]);
    assert!(ok, "{err}");
    assert!(brief.contains(&format!("{a} Measure")), "{brief}");
    assert!(brief.contains("result: 24/30 · date: 20"), "{brief}");
}
