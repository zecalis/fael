//! `fael chunk status <uid>` (f6): both sides of a joint run at once —
//! branch/base/ahead per side plus unstaged. DB from the store, git live in
//! the CLI; read-only, no state change.

use std::path::{Path, PathBuf};
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

fn git(d: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(d)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-status-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/x.rs"), "one\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "init"]);
    let (ok, _, err) = fael(&d, &["plan", "new", "pl", "PL"]);
    assert!(ok, "{err}");
    d
}

fn chunk(d: &Path, title: &str) -> String {
    let (ok, uid, err) = fael(
        d,
        &[
            "chunk", "add", title, "--plan", "pl", "--brief", "b", "--scope", "src/x.rs",
        ],
    );
    assert!(ok, "{err}");
    uid.trim().to_string()
}

/// A pair's status names both sides, each with branch/base/ahead + unstaged.
#[test]
fn pair_status_shows_both_sides_with_git_reads() {
    let d = repo();
    let a = chunk(&d, "a2 work");
    let b = chunk(&d, "a4 work");
    let (ok, _, err) = fael(&d, &["chunk", "start", &a]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["chunk", "start", &b, "--with", &a, "--as", "pair"]);
    assert!(ok, "{err}");
    std::fs::write(d.join("src/x.rs"), "one\ntwo\n").unwrap();
    git(&d, &["add", "src/x.rs"]);
    git(&d, &["commit", "-qm", "a2: touch x"]);
    std::fs::write(d.join("src/x.rs"), "one\ntwo\nunstaged\n").unwrap();

    let (ok, out, err) = fael(&d, &["chunk", "status", &a]);
    assert!(ok, "{err}");
    for needle in [
        &a,
        &b,
        "branch",
        "base",
        "ahead 1",
        "a2: touch x",
        "M src/x.rs",
    ] {
        assert!(out.contains(needle), "{needle} missing in:\n{out}");
    }
    // either side answers the same joint run
    let (ok, out_b, err) = fael(&d, &["chunk", "status", &b]);
    assert!(ok, "{err}");
    assert!(out_b.contains(&a) && out_b.contains(&b), "{out_b}");

    // --json carries both uids with their git reads
    let (ok, out, err) = fael(&d, &["chunk", "status", &a, "--json"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("status --json");
    let uids: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["uid"].as_str().unwrap())
        .collect();
    assert!(
        uids.contains(&a.as_str()) && uids.contains(&b.as_str()),
        "{uids:?}"
    );
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .all(|s| s["ahead"].as_array().is_some_and(|a| a.len() == 1)),
        "{v}"
    );

    // read-only: both chunks still run
    let (ok, out, err) = fael(&d, &["chunk", "show", &a]);
    assert!(ok, "{err}");
    assert!(out.contains("running"), "{out}");
}

/// A solo chunk is one side; an unknown uid rejects like show does.
#[test]
fn solo_status_is_one_side_and_unknown_rejects() {
    let d = repo();
    let c = chunk(&d, "solo");
    let (ok, out, err) = fael(&d, &["chunk", "status", &c]);
    assert!(ok, "{err}");
    assert!(
        out.contains("status 1 side(s)") && out.contains(&c),
        "{out}"
    );
    let (ok, _, err) = fael(&d, &["chunk", "status", "01M6ZZZZZZZZZZZZZZZZZZZZZZ"]);
    assert!(!ok, "unknown uid accepted");
    assert!(err.contains("no chunk"), "{err}");
}

fn fael_at(wt: &Path, state: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(crate::common::fael_bin())
        .args(args)
        .current_dir(wt)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// Two chunks with disjoint scopes (the start gate allows it), each started in its own
/// linked worktree: (repo, wt1, wt2, uid1, uid2). `x.txt` has 30 lines.
fn two() -> (PathBuf, PathBuf, PathBuf, String, String) {
    let d = repo();
    let body: String = (1..=30).map(|i| format!("line{i}\n")).collect();
    std::fs::write(d.join("x.txt"), body).unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "x"]);
    let (w1, w2) = (d.join("w1"), d.join("w2"));
    git(
        &d,
        &["worktree", "add", "-q", "-b", "b1", w1.to_str().unwrap()],
    );
    git(
        &d,
        &["worktree", "add", "-q", "-b", "b2", w2.to_str().unwrap()],
    );
    let state = d.join("state");
    let mut u = Vec::new();
    for (n, wt) in [("a", &w1), ("b", &w2)] {
        let sc = format!("src/{n}.rs");
        let (ok, uid, err) = fael_at(
            &d,
            &state,
            &[
                "chunk", "add", n, "--plan", "pl", "--brief", "b", "--scope", &sc,
            ],
        );
        assert!(ok, "{err}");
        let uid = uid.trim().to_string();
        let (ok, _, err) = fael_at(wt, &state, &["chunk", "start", &uid]);
        assert!(ok, "{err}");
        u.push(uid);
    }
    (d, w1, w2, u.remove(0), u.remove(0))
}

fn edit(wt: &Path, line: usize, to: &str) {
    let s = std::fs::read_to_string(wt.join("x.txt")).unwrap();
    let mut v: Vec<&str> = s.lines().collect();
    v[line - 1] = to;
    std::fs::write(wt.join("x.txt"), v.join("\n") + "\n").unwrap();
}

fn status(d: &Path, wt: &Path, uid: &str) -> String {
    let (ok, out, err) = fael_at(wt, &d.join("state"), &["chunk", "status", uid]);
    assert!(ok, "{err}");
    out
}

/// Far-apart uncommitted edits to one file merge clean: nothing said.
#[test]
fn far_apart_edits_are_clean() {
    let (d, w1, w2, a, _) = two();
    edit(&w1, 2, "mine");
    edit(&w2, 28, "theirs");
    assert!(!status(&d, &w1, &a).contains("conflict"));
}

/// Same line, uncommitted on both sides: names the chunk, file, diff and note commands.
#[test]
fn same_line_conflicts_with_ready_commands() {
    let (d, w1, w2, a, b) = two();
    edit(&w1, 5, "mine");
    git(&w1, &["commit", "-qam", "m"]);
    edit(&w2, 5, "theirs"); // uncommitted
    let out = status(&d, &w1, &a);
    let diff = format!(
        "git -C {} diff -- x.txt",
        w2.canonicalize().unwrap().display()
    );
    for n in [
        &format!("conflict with {b}"),
        "branch b2",
        "x.txt",
        &diff,
        &format!("fael chunk note {b} \""),
    ] {
        assert!(out.contains(n), "{n} missing in:\n{out}");
    }
    let (ok, j, err) = fael_at(&w1, &d.join("state"), &["chunk", "status", &a, "--json"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&j).unwrap();
    let me = v
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["uid"] == a.as_str())
        .unwrap();
    assert_eq!(me["conflicts"][0]["files"][0], "x.txt", "{v}");
}

/// The same new filename on both sides, untracked, conflicts.
#[test]
fn same_untracked_filename_conflicts() {
    let (d, w1, w2, a, _) = two();
    std::fs::write(w1.join("new.txt"), "one\n").unwrap();
    std::fs::write(w2.join("new.txt"), "two\n").unwrap();
    assert!(status(&d, &w1, &a).contains("new.txt"));
}

/// A joint partner in another worktree can really conflict, so it is checked like any run
/// (issue 01M6KTTC): both modes, both directions.
#[test]
fn a_partner_in_another_worktree_is_checked() {
    for mode in ["lane", "follow"] {
        let (d, w1, _, a, _) = two();
        let state = d.join("state");
        let w3 = d.join("w3");
        git(
            &d,
            &["worktree", "add", "-q", "-b", "b3", w3.to_str().unwrap()],
        );
        let (ok, c, err) = fael_at(
            &d,
            &state,
            &[
                "chunk", "add", "c", "--plan", "pl", "--brief", "b", "--scope", "src/c.rs",
            ],
        );
        assert!(ok, "{err}");
        let c = c.trim().to_string();
        let (ok, _, err) = fael_at(
            &w3,
            &state,
            &["chunk", "start", &c, "--with", &a, "--as", mode],
        );
        assert!(ok, "{mode}: {err}");
        edit(&w1, 5, "mine");
        edit(&w3, 5, "theirs");
        let want = |u: &str| format!("conflict with {u}");
        let out = status(&d, &w3, &c);
        assert!(out.contains(&want(&a)), "{mode}: {out}");
        let out = status(&d, &w1, &a);
        assert!(out.contains(&want(&c)), "{mode}: {out}");
    }
}

/// Diff-aware status (issue 01M6Z008): observed worktree files outside the declared
/// scope are named before done claims them — committed and unstaged alike.
#[test]
fn status_names_observed_outside_scope_before_done() {
    let d = repo();
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(
        &d,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    let c = chunk(&d, "observed work");
    let (ok, _, err) = fael(&d, &["chunk", "start", &c]);
    assert!(ok, "{err}");
    git(&d, &["switch", "-q", "-c", "lane"]);
    std::fs::write(d.join("stray.txt"), "stray\n").unwrap();
    git(&d, &["add", "stray.txt"]);
    git(&d, &["commit", "-qm", "stray"]);
    std::fs::write(d.join("unstaged.txt"), "u\n").unwrap();
    let (ok, out, err) = fael(&d, &["chunk", "status", &c]);
    assert!(ok, "{err}");
    // `state/` is the test harness's own FAEL_STATE_DIR inside the repo: it also
    // reads as observed, so assert per-file, never an exact join.
    for needle in [
        "outside declared scope: stray.txt",
        "outside declared scope: unstaged.txt",
    ] {
        assert!(out.contains(needle), "{needle} missing in:\n{out}");
    }
    let widen = out
        .lines()
        .find(|l| l.contains("to cover them:"))
        .unwrap_or_default()
        .to_string();
    assert!(
        widen.contains("+stray.txt") && widen.contains("+unstaged.txt"),
        "{widen}: {out}"
    );
    let (ok, j, err) = fael(&d, &["chunk", "status", &c, "--json"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&j).unwrap();
    let me = &v.as_array().unwrap()[0];
    let observed: Vec<&str> = me["observed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x.as_str())
        .collect();
    assert!(
        observed.contains(&"stray.txt") && observed.contains(&"unstaged.txt"),
        "{v}"
    );
    let files: Vec<&str> = me["outside"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["file"].as_str())
        .collect();
    assert!(
        files.contains(&"stray.txt") && files.contains(&"unstaged.txt"),
        "{v}"
    );
}

/// A neighbour whose worktree is gone: the command still succeeds and says so.
#[test]
fn missing_neighbour_worktree_is_unchecked_not_a_failure() {
    let (d, w1, w2, a, b) = two();
    std::fs::remove_dir_all(&w2).unwrap();
    let out = status(&d, &w1, &a);
    assert!(out.contains(&format!("could not check {b}")), "{out}");
}

/// Checking leaves the neighbour's `git status` and index untouched.
#[test]
fn check_leaves_neighbour_git_status_unchanged() {
    let (d, w1, w2, a, _) = two();
    edit(&w1, 5, "mine");
    edit(&w2, 5, "theirs");
    std::fs::write(w2.join("new.txt"), "n\n").unwrap();
    git(&w2, &["add", "x.txt"]);
    let st = |w: &Path| {
        Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(w)
            .output()
            .unwrap()
            .stdout
    };
    let (before, before1) = (st(&w2), st(&w1));
    assert!(status(&d, &w1, &a).contains("conflict"));
    assert_eq!((st(&w2), st(&w1)), (before, before1));
}

/// No uid reads this worktree's live run; none or two reject with the retype.
#[test]
fn status_without_uid_reads_the_worktrees_live_run() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["chunk", "status"]);
    assert!(!ok && err.contains("fael chunk status <uid>"), "{err}");
    let c = chunk(&d, "solo here");
    let (ok, _, err) = fael(&d, &["chunk", "start", &c]);
    assert!(ok, "{err}");
    let (ok, out, err) = fael(&d, &["chunk", "status"]);
    assert!(ok && out.contains(&c), "{err}{out}");
    let e = chunk(&d, "second");
    let (ok, _, err) = fael(&d, &["chunk", "start", &e, "--with", &c, "--as", "pair"]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["chunk", "status"]);
    assert!(!ok && err.contains(&c) && err.contains(&e), "{err}");
}

/// pr-cross: `status` names a done chunk's still-open PR touching this run's files.
#[test]
fn status_names_open_done_prs_touching_same_files() {
    let d = repo();
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(
        &d,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    let a = chunk(&d, "a work");
    let b = chunk(&d, "b work");
    let (ok, _, err) = fael(&d, &["chunk", "start", &a]);
    assert!(ok, "{err}");
    git(&d, &["switch", "-q", "-c", "lane-a"]);
    std::fs::write(d.join("src/x.rs"), "a\n").unwrap();
    git(&d, &["add", "src/x.rs"]);
    git(&d, &["commit", "-qm", "a touch"]);
    let (ok, _, err) = fael(
        &d,
        &["chunk", "done", &a, "did a", "--left", "none", "--pr", "7"],
    );
    assert!(ok, "{err}");
    git(&d, &["switch", "-q", "main"]);
    let (ok, _, err) = fael(&d, &["chunk", "start", &b]);
    assert!(ok, "{err}");
    let out = Command::new(crate::common::fael_bin())
        .args(["chunk", "status", &b])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env("FAEL_GH_PR_FILES", "src/x.rs")
        .env_remove("FAEL_DIR")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.contains(&format!("also in PR #7 ({a}): src/x.rs")),
        "{out}"
    );
}
