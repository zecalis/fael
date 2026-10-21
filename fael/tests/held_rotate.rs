//! Issue 01M5EEGR: `fael held done|start` rotate the rows they name across
//! sessions, start names at most 3, and the `+N more` tail is never stamped.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn run(d: &Path, s: &Path, session: &str, args: &[&str], stdin: &str) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", s)
        .env("FAEL_SESSION", session)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(d)
        .status()
        .unwrap();
    assert!(ok.success(), "git {args:?}");
}

/// 7 held notes (two files each, `a.rs` changed, `b.rs` not).
fn repo() -> (PathBuf, PathBuf, Vec<String>) {
    let t = std::env::temp_dir().join(format!("fael-rot-{}", fael_core::ulid()));
    let d = t.join("r");
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    std::fs::write(d.join("src/a.rs"), "fn x() {}\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "fn y() {}\n").unwrap();
    let (d, s) = (d.canonicalize().unwrap(), t.join("state"));
    let ids = (0..7)
        .map(|i| {
            let out = run(
                &d,
                &s,
                "W",
                &[
                    "add",
                    "note",
                    &format!("rule {i}"),
                    "--files",
                    "src/a.rs,src/b.rs",
                ],
                "",
            );
            out.split_whitespace().next().unwrap().to_string()
        })
        .collect();
    std::fs::write(d.join("src/a.rs"), "fn x() { 1 }\n").unwrap();
    (d, s, ids)
}

fn named<'a>(out: &str, ids: &'a [String]) -> Vec<&'a String> {
    // each held line reads `fael: note <short id> — ...`
    let shorts: Vec<&str> = out
        .lines()
        .filter_map(|l| l.strip_prefix("fael: note "))
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    ids.iter()
        .filter(|i| shorts.iter().any(|s| i.starts_with(s)))
        .collect()
}

#[test]
fn done_rotates_across_sessions_and_start_names_three() {
    let (d, s, ids) = repo();
    let one = named(&run(&d, &s, "S1", &["held", "done"], "src/a.rs\n"), &ids)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(one.len(), 5);
    let out2 = run(&d, &s, "S2", &["held", "done"], "src/a.rs\n");
    let two = named(&out2, &ids);
    assert_eq!(two.len(), 5);
    // 7 rows, 5 shown first: the second call names the 2 never-shown rows first
    assert_eq!(
        two.iter().filter(|i| !one.contains(**i)).count(),
        2,
        "{out2}"
    );
    assert!(out2.contains("+2 more"), "{out2}");
    let start = run(&d, &s, "S3", &["held", "start"], "src/\n");
    assert_eq!(named(&start, &ids).len(), 3, "{start}");
    assert!(start.contains("+4 more"), "{start}");
}

#[test]
fn the_more_tail_is_not_stamped() {
    let (d, s, ids) = repo();
    let out = run(&d, &s, "S1", &["held", "done"], "src/a.rs\n");
    let shown = named(&out, &ids);
    let rec = std::fs::read_to_string(
        std::fs::read_dir(s.join("held-shown"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap();
    assert_eq!(rec.lines().count(), 5, "{rec}");
    assert!(
        ids.iter()
            .filter(|i| !shown.contains(i))
            .all(|i| !rec.contains(i.as_str()))
    );
}

#[test]
fn done_states_the_evidence_under_each_row() {
    let (d, s, _) = repo();
    git(&d, &["add", "src/a.rs"]);
    git(&d, &["commit", "-qm", "touch a"]);
    let out = run(&d, &s, "S1", &["held", "done"], "src/a.rs\n");
    assert!(
        out.contains("1/2 files changed, still matches: src/b.rs"),
        "{out}"
    );
    assert!(out.contains("1 commit(s) on `src/a.rs`"), "{out}");
    assert!(
        out.contains("\"touch a\"") && out.contains("bumped 0×"),
        "{out}"
    );
    assert!(
        out.contains("inspect: fael find") && out.contains("git log --oneline -5 -- src/a.rs"),
        "{out}"
    );
    let per_row = out.matches("inspect:").count();
    assert_eq!(per_row, 5, "{out}");
}
