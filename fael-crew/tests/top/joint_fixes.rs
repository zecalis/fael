//! Lane/pair seams found by hand (issues 01M6E23QPV, 01M6E23QTD, 01M6E23QX4): one git
//! worktree per running chunk, every call made from its own worktree.

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

/// A repo with one commit and the worktrees `names`; returns them in order.
fn worktrees(names: &[&str]) -> Vec<PathBuf> {
    let d = std::env::temp_dir().join(format!("fael-joint-{}", fael_core::ulid()));
    let main = d.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
    names
        .iter()
        .map(|n| {
            let w = d.join(n);
            // git on Windows rejects the `\\?\` verbatim prefix `canonicalize` adds.
            let arg = w.to_string_lossy().replacen(r"\\?\", "", 1);
            git(&main, &["worktree", "add", "-q", "-b", n, &arg]);
            w.canonicalize().unwrap()
        })
        .collect()
}

/// A chunk whose scope is `src/x.rs`.
fn chunk(w: &Path, title: &str) -> String {
    let (ok, uid, err) = fael(
        w,
        &["chunk", "add", title, "--brief", "b", "--scope", "src/x.rs"],
    );
    assert!(ok, "{err}");
    uid.trim().to_string()
}

fn ok(w: &Path, args: &[&str]) -> String {
    let (good, out, err) = fael(w, args);
    assert!(good, "{args:?}: {out}{err}");
    out
}

/// Issue 01M6E23QPV: `touch --msg` reached the arg parser as an unknown flag.
#[test]
fn touch_msg_shows_as_doing_on_the_other_cards() {
    let w = worktrees(&["wa", "wb"]);
    let (a, b) = (chunk(&w[0], "A"), chunk(&w[0], "B"));
    ok(&w[0], &["chunk", "start", &a]);
    ok(
        &w[0],
        &[
            "chunk",
            "touch",
            &a,
            "src/x.rs",
            "--msg",
            "rewriting parse()",
        ],
    );
    let shown = ok(&w[1], &["chunk", "show", &b]);
    assert!(shown.contains("doing: rewriting parse()"), "{shown}");
}

/// Issue 01M6HA52P (f6): `touch --status` stores the agent's git snapshot in
/// the touch event tied to the run, and prints it back — the lost-hunk audit.
#[test]
fn touch_status_is_stored_and_printed() {
    let w = worktrees(&["wa"]);
    let a = chunk(&w[0], "A");
    ok(&w[0], &["chunk", "start", &a]);
    let (good, out, err) = fael(
        &w[0],
        &["chunk", "touch", &a, "src/x.rs", "--status", " M src/x.rs"],
    );
    assert!(good, "{out}{err}");
    assert!(out.contains(" M src/x.rs"), "prints the snapshot: {out}");
    // the audit path: the change feed carries the touch event with the snapshot
    let feed = ok(&w[0], &["plan", "export", "--since", "0"]);
    assert!(feed.contains(" M src/x.rs"), "feed audits it: {feed}");
    assert!(feed.contains("status:"), "tied to the run: {feed}");
}

/// A pair's brief offered `start --with <partner> --as pair` — the step taken.
#[test]
fn a_pair_partner_is_no_avoid_card() {
    let w = worktrees(&["wa", "wb"]);
    let (a, b) = (chunk(&w[0], "A"), chunk(&w[0], "B"));
    ok(&w[0], &["chunk", "start", &a]);
    let start = ok(&w[1], &["chunk", "start", &b, "--with", &a, "--as", "pair"]);
    assert!(!start.contains("avoid:"), "{start}");
    let shown = ok(&w[0], &["chunk", "show", &b]);
    assert!(!shown.contains("avoid:"), "{shown}");
}

/// Issue 01M6E23QX4: D pairing with A was refused for B's clash, though B works inside A's run.
#[test]
fn a_pair_start_clears_clashes_under_the_partners_run() {
    let w = worktrees(&["wa", "wb", "wd"]);
    let (a, b, d) = (chunk(&w[0], "A"), chunk(&w[0], "B"), chunk(&w[0], "D"));
    ok(&w[0], &["chunk", "start", &a]);
    ok(&w[1], &["chunk", "start", &b, "--with", &a, "--as", "pair"]);
    let (good, out, err) = fael(&w[2], &["chunk", "start", &d, "--with", &a, "--as", "pair"]);
    assert!(good, "{out}{err}");
}

/// Issue 01M6KHWF: a scope overlap starts, says who and the ready note, and refuses nothing.
#[test]
fn an_overlap_starts_and_names_who_to_note() {
    let w = worktrees(&["wa", "wb", "wc"]);
    let (a, b) = (chunk(&w[0], "A"), chunk(&w[0], "B"));
    ok(&w[0], &["chunk", "start", &a]);
    let out = ok(&w[1], &["chunk", "start", &b]);
    assert!(out.contains(&format!("avoid: {a}")), "{out}");
    assert!(out.contains(&format!("fael chunk note {a} ")), "{out}");
    assert!(out.contains("fael chunk status"), "{out}");
    assert!(
        !out.contains("--with"),
        "no joint offered as the way out: {out}"
    );
    // `--with --as lane` over the same file still works, and prints the lane rules
    let c = chunk(&w[0], "C");
    let args = [
        "chunk", "start", &c, "--run", "RC", "--with", &a, "--as", "lane",
    ];
    let out = ok(&w[2], &args);
    assert!(out.contains("lane: one scope per worktree"), "{out}");
    assert!(out.contains("`fael run end RC`"), "the run named: {out}");
}

/// The one refusal left: another live run in the same worktree (two agents overwrite each other).
#[test]
fn another_live_run_in_the_same_worktree_refuses_until_forced() {
    let w = worktrees(&["wa"]);
    let (a, b) = (chunk(&w[0], "A"), chunk(&w[0], "B"));
    ok(&w[0], &["chunk", "start", &a]);
    let (good, _, err) = fael(&w[0], &["chunk", "start", &b]);
    assert!(
        !good
            && err.contains(&a)
            && err.contains("another worktree")
            && err.contains("--force --because"),
        "{err}"
    );
    let out = ok(
        &w[0],
        &["chunk", "start", &b, "--force", "--because", "a is idle"],
    );
    assert!(out.contains("a is idle"), "{out}");
}

/// The pair start said "pass --run R to done", but done from the partner's worktree needs none.
#[test]
fn a_pair_done_from_the_partners_worktree_needs_no_run() {
    let w = worktrees(&["wa", "wb"]);
    let (a, b) = (chunk(&w[0], "A"), chunk(&w[0], "B"));
    ok(&w[0], &["chunk", "start", &a]);
    let start = ok(&w[1], &["chunk", "start", &b, "--with", &a, "--as", "pair"]);
    assert!(!start.contains("--run"), "{start}");
    let done = ["chunk", "done", &b, "h", "--left", "none", "--no-change"];
    ok(&w[0], &done);
}
