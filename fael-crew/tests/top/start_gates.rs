//! The seams `chunk start` and `chunk done` close without an LLM (issue 01M5FMHQX): a brief
//! citing a closed row is refused before it claims, and `done` names what it leaves.

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

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-gates-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    let init = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&d)
        .status()
        .unwrap();
    assert!(init.success());
    assert!(fael(&d, &["plan", "new", "pl", "PL"]).0);
    d
}

fn chunk(d: &Path, title: &str, brief: &str, more: &[&str]) -> String {
    let args = [
        &["chunk", "add", title, "--plan", "pl", "--brief", brief][..],
        more,
    ]
    .concat();
    let (ok, uid, err) = fael(d, &args);
    assert!(ok, "{err}");
    uid.trim().to_string()
}

fn issue(d: &Path, text: &str) -> String {
    let (ok, out, err) = fael(d, &["add", "issue", text, "--files", "a.rs"]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
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

fn is_ancestor(d: &Path, tip: &str, head: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", tip, head])
        .current_dir(d)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A repo with one commit plus the worktrees `names`; returns (main, worktrees).
fn stack_tree(names: &[&str]) -> (PathBuf, Vec<PathBuf>) {
    let d = std::env::temp_dir().join(format!("fael-stack-{}", fael_core::ulid()));
    let main = d.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
    assert!(fael(&main, &["plan", "new", "pl", "PL"]).0);
    let mut out = Vec::new();
    for n in names {
        let w = d.join(n);
        let arg = w.to_string_lossy().replacen(r"\\?\", "", 1);
        git(&main, &["worktree", "add", "-q", "-b", n, &arg]);
        out.push(w.canonicalize().unwrap());
    }
    (main.canonicalize().unwrap(), out)
}

#[test]
fn a_note_naming_the_closed_row_cures_a_brief_cite_without_force() {
    let d = repo();
    let id = issue(&d, "broken x");
    assert!(
        fael(
            &d,
            &["close", &id, "fixed in 5 → done; guard `src/a_test.rs`"]
        )
        .0
    );
    let u = chunk(&d, "Fix", &format!("fix {id} (review {id})"), &[]);
    let (ok, _, err) = fael(&d, &["chunk", "start", &u, "--no-worktree"]);
    assert!(!ok, "started without acknowledgement: {err}");
    assert!(
        fael(
            &d,
            &[
                "chunk",
                "note",
                &u,
                &format!("read {id}: provenance, still starts")
            ]
        )
        .0
    );
    let (ok, _, err) = fael(&d, &["chunk", "start", &u, "--no-worktree"]);
    assert!(ok, "{err}");
}

#[test]
fn start_refuses_a_brief_citing_a_closed_row_until_forced() {
    let d = repo();
    let id = issue(&d, "broken x");
    assert!(
        fael(
            &d,
            &[
                "close",
                &id,
                "fixed in 5 → done; guard `src/a_test.rs`\nmore"
            ]
        )
        .0
    );
    let u = chunk(&d, "Fix", &format!("fix {id}"), &[]);
    let (ok, _, err) = fael(&d, &["chunk", "start", &u, "--no-worktree"]);
    assert!(!ok, "started");
    assert!(err.contains(&format!("{id} — closed: fixed in 5")), "{err}");
    assert!(!err.contains("more"), "first line only: {err}");
    assert!(
        err.contains(&format!("fael chunk note {u}")) && err.contains("never `chunk edit`"),
        "{err}"
    );
    let (_, shown, _) = fael(&d, &["chunk", "show", &u, "--json"]);
    assert!(
        shown.contains("\"state\": \"open\"") || shown.contains("\"state\":\"open\""),
        "claimed: {shown}"
    );
    // show only warns
    assert!(fael(&d, &["chunk", "show", &u]).0);
    let (ok, _, err) = fael(&d, &["chunk", "start", &u, "--no-worktree", "--force"]);
    assert!(
        !ok && err.contains("--because"),
        "bare --force rejected: {err}"
    );
    let (ok, _, err) = fael(
        &d,
        &[
            "chunk",
            "start",
            &u,
            "--no-worktree",
            "--force",
            "--because",
            "correction noted, will verify",
        ],
    );
    assert!(ok, "{err}");
}

/// A blocked or not-ready start prints one exact next call with the real uid:
/// a blocked start keeps its `--force --because` retype, and a draft names the
/// owner or planner — never a placeholder, never an agent `try edit`.
#[test]
fn a_blocked_or_draft_start_retype_carries_the_real_uid() {
    let d = repo();
    let a = chunk(&d, "A", "do a", &[]);
    let b = chunk(&d, "B", "do b", &["--after", &a]);
    let (ok, _, err) = fael(&d, &["chunk", "start", &b, "--no-worktree"]);
    assert!(!ok, "blocked started");
    assert!(
        err.contains(&format!("fael chunk start {b} --force --because")),
        "{err}"
    );
    assert!(!err.contains("<uid>"), "{err}");
    // a draft (no brief) names who makes it startable, with the real uid
    let (ok, draft, err) = fael(&d, &["chunk", "add", "Draft", "--plan", "pl"]);
    assert!(ok, "{err}");
    let draft = draft.trim().to_string();
    let (ok, _, err) = fael(&d, &["chunk", "start", &draft, "--no-worktree"]);
    assert!(!ok, "draft started");
    assert!(err.contains("owner or planner"), "{err}");
    assert!(!err.contains("try `fael chunk edit"), "{err}");
    assert!(
        err.contains(&format!("fael chunk edit {draft} --brief"))
            && err.contains(&format!("fael chunk start {draft}")),
        "{err}"
    );
}

#[test]
fn done_names_its_leftovers_and_the_next_brief_prints_them() {
    let d = repo();
    let a = chunk(&d, "A", "do a", &[]);
    let b = chunk(&d, "B", "do b", &["--after", &a]);
    let open = issue(&d, "still broken");
    let shut = issue(&d, "was broken");
    assert!(
        fael(
            &d,
            &["close", &shut, "was broken → fixed; guard `src/a_test.rs`"]
        )
        .0
    );
    assert!(fael(&d, &["chunk", "start", &a, "--no-worktree", "--run", "R1"]).0);
    let done = |h: &str, left: &[&str]| {
        let args = [
            &["chunk", "done", &a, h, "--run", "R1", "--pr", "7"][..],
            left,
        ]
        .concat();
        fael(&d, &args)
    };
    let (ok, _, err) = done("h", &[]);
    assert!(
        !ok && err.contains("needs --left") && err.contains("fael add issue"),
        "{err}"
    );
    let (ok, _, err) = done("h", &["--left", &shut]);
    assert!(
        !ok && err.contains(&shut),
        "closed row is no leftover: {err}"
    );
    let (ok, _, err) = done("h", &["--left", "none,nope"]);
    assert!(!ok, "{err}");
    let list = format!("{open},{b}");
    let (ok, _, err) = done("shipped", &["--left", &list]);
    assert!(ok, "{err}");
    let (ok, brief, err) = fael(&d, &["chunk", "start", &b, "--no-worktree"]);
    assert!(ok, "{err}");
    assert!(
        brief.contains("shipped") && brief.contains(&format!("left: {open},{b}")),
        "{brief}"
    );
}

#[test]
fn a_finished_note_carries_the_statement_as_a_left_line() {
    let d = repo();
    let a = chunk(&d, "A", "do a", &[]);
    assert!(fael(&d, &["chunk", "start", &a, "--no-worktree", "--run", "R1"]).0);
    let (ok, _, err) = fael(
        &d,
        &[
            "chunk",
            "done",
            &a,
            "x\nleft: none",
            "--run",
            "R1",
            "--pr",
            "7",
        ],
    );
    assert!(ok, "{err}");
    let (_, j, _) = fael(&d, &["chunk", "show", &a, "--json"]);
    assert!(j.contains("done"), "{j}");
    let (ok, _, err) = fael(
        &d,
        &["chunk", "done", &a, "x\nleft: none", "--left", "none"],
    );
    assert!(!ok && err.contains("once"), "{err}");
}

/// A stacked start is proven on a real tree: B cut from A's branch starts,
/// its HEAD containing A's tip; a start whose HEAD lacks the tip is rejected.
#[test]
fn stack_start_needs_partner_tip_on_a_real_tree() {
    let (main, w) = stack_tree(&["wa", "wb"]);
    let (wa, wb) = (&w[0], &w[1]);
    let a = chunk(&main, "A", "do a", &[]);
    let b = chunk(&main, "B", "do b", &[]);
    assert!(fael(wa, &["chunk", "start", &a]).0);
    git(wa, &["commit", "-q", "--allow-empty", "-m", "a-tip"]);
    let tip = Command::new("git")
        .args(["rev-parse", "wa"])
        .current_dir(wb)
        .output()
        .unwrap();
    let tip = String::from_utf8_lossy(&tip.stdout).trim().to_string();
    assert!(!is_ancestor(wb, &tip, "HEAD"), "wb lacks A's tip");
    let (ok, _, err) = fael(wb, &["chunk", "start", &b, "--with", &a, "--as", "stack"]);
    assert!(!ok, "stack started without the tip: {err}");
    assert!(err.contains("stacks on") && err.contains(&a), "{err}");
    assert!(err.contains("does not contain"), "{err}");
    let wc = main.parent().unwrap().join("wc");
    let arg = wc.to_string_lossy().replacen(r"\\?\", "", 1);
    git(&main, &["worktree", "add", "-q", "-b", "wc", &arg, "wa"]);
    let wc = wc.canonicalize().unwrap();
    assert!(is_ancestor(&wc, &tip, "HEAD"), "wc holds A's tip");
    let (ok, out, err) = fael(&wc, &["chunk", "start", &b, "--with", &a, "--as", "stack"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains(&format!("joint stack with {a}")), "{out}");
    assert!(out.contains("stack:"), "{out}");
}

/// Fixture for the start golden: a repo with one file, plan `pl` whose
/// Scope section carries a fixed body (the one printed plan section).
fn golden_repo() -> (PathBuf, PathBuf) {
    let tmp = std::env::temp_dir().join(format!("fael-start-golden-{}", fael_core::ulid()));
    let d = tmp.join("repo");
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("src/a.rs"), "v1\n").unwrap();
    git(&d, &["add", "src/a.rs"]);
    git(&d, &["commit", "-qm", "init"]);
    assert!(fael(&d, &["plan", "new", "pl", "Plan Title"]).0);
    assert!(
        fael(
            &d,
            &[
                "plan",
                "edit",
                "pl",
                "--section",
                "2",
                "--replace",
                "**Do:**",
                "--with",
                "**Do:** one thing."
            ]
        )
        .0
    );
    (tmp, d)
}

/// The golden's rows: one open + one closed-with-note cite, one linked issue,
/// one `after` chunk done with a PR number. Returns (open, shut, link, a, b).
fn golden_chunks(d: &Path) -> (String, String, String, String, String) {
    let open = issue(d, "open problem");
    let shut = issue(d, "was broken");
    assert!(
        fael(
            d,
            &["close", &shut, "was broken → fixed; guard `src/a_test.rs`"]
        )
        .0
    );
    let link = issue(d, "linked bug");
    let a = chunk(d, "Prev", "prev work", &[]);
    assert!(fael(d, &["chunk", "start", &a, "--no-worktree", "--run", "R0"]).0);
    assert!(
        fael(
            d,
            &[
                "chunk",
                "done",
                &a,
                "prev shipped",
                "--left",
                "none",
                "--pr",
                "7",
                "--run",
                "R0"
            ]
        )
        .0
    );
    let b = chunk(
        d,
        "Fix",
        &format!("fix {open} (review {shut})"),
        &["--scope", "src/a.rs", "--issue", &link, "--after", &a],
    );
    assert!(
        fael(
            d,
            &[
                "chunk",
                "note",
                &b,
                &format!("read {shut}: provenance, still starts")
            ]
        )
        .0
    );
    (open, shut, link, a, b)
}

/// The whole `chunk start` output pinned by a golden file: a fixture chunk
/// whose brief cites one open and one closed-with-note row, one linked issue,
/// one `after` chunk with a done text and one filled plan section.
/// Re-bless on purpose with `FAEL_BLESS=1`; production code stays untouched.
#[test]
fn start_output_matches_golden() {
    let snap = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/start-v1.txt");
    let (tmp, d) = golden_repo();
    let (open, shut, link, a, b) = golden_chunks(&d);
    let (ok, out, err) = fael(&d, &["chunk", "start", &b, "--no-worktree", "--full"]);
    assert!(ok, "{err}");
    let run = out
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .nth(2)
        .unwrap_or_default()
        .to_string();
    let ids = [
        (open, "<OPEN>"),
        (shut, "<SHUT>"),
        (link, "<LINK>"),
        (a, "<CHUNK-A>"),
        (b, "<CHUNK-B>"),
        (run, "<RUN>"),
    ];
    let owned: Vec<(String, &str)> = ids.into_iter().collect();
    let got = crate::common::normalise_start(out.trim(), &tmp, &owned) + "\n";
    if std::env::var_os("FAEL_BLESS").is_some() {
        std::fs::create_dir_all(std::path::Path::new(snap).parent().unwrap()).unwrap();
        std::fs::write(snap, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(snap).unwrap_or_default();
    assert!(
        got == want,
        "golden start drifted — FAEL_BLESS=1 to re-bless on purpose\n--- got\n{got}"
    );
}
