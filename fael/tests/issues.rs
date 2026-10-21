//! Working the issue list in the real binary: waiting issues list last,
//! `claim` shows who holds one, and
//! `--full` asks for the rest in one call.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"));
    let o = c.output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn git(d: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(d)
            .status()
            .unwrap()
            .success()
    );
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-issues-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q", "-b", "feat/one"]);
    git(&d, &["config", "user.name", "Test User"]);
    git(&d, &["config", "user.email", "t@example.com"]);
    for f in ["a", "b", "c"] {
        std::fs::write(d.join(format!("src/{f}.rs")), "//\n").unwrap();
    }
    d
}

/// Add an issue, return its id.
fn issue(d: &Path, text: &str, files: &str, extra: &[&str]) -> (String, String) {
    let mut args = vec!["add", "issue", text, "--files", files, "--json", "--force"];
    args.extend(extra);
    let (ok, out, err) = fael(d, &args);
    assert!(ok, "{err}");
    let row: serde_json::Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    (row["id"].as_str().unwrap().to_string(), err)
}

#[test]
fn waiting_issues_list_after_ready_ones() {
    let d = repo();
    let (_, warn) = issue(&d, "ocr retry WHEN: vendor fixes 5xx", "src/a.rs", &[]);
    assert!(
        warn.contains("WHEN:") && warn.contains("--revisit"),
        "{warn}"
    );
    issue(
        &d,
        "ocr gated",
        "src/a.rs",
        &["--revisit", "vendor fixes 5xx"],
    );
    issue(&d, "scope bug", "src/b.rs", &[]);
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue"]);
    assert!(ok, "{err}");
    // ready first, waiting last, as one flat list
    let ready = out.find("scope bug").unwrap();
    assert!(ready < out.find("ocr gated").unwrap(), "{out}");
    assert!(
        out.contains("ocr gated (waiting: vendor fixes 5xx)"),
        "{out}"
    );
    assert!(!out.contains("## group"), "{out}");
}

#[test]
fn claim_shows_the_holding_branch() {
    let d = repo();
    // a branch exists once it has a commit — a hold on a live branch is real
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    let (id, _) = issue(&d, "invite accept breaks", "src/a.rs", &[]);
    let (ok, _, err) = fael(&d, &["claim", &id]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["find", "--kind", "issue"]);
    assert!(
        out.contains("invite accept breaks (held @feat/one)"),
        "{out}"
    );
    // the same branch again: nothing to do
    let new = out.split("- [").nth(1).unwrap().split(']').next().unwrap();
    let (ok, _, err) = fael(&d, &["claim", new]);
    assert!(!ok && err.contains("already held @feat/one"), "{err}");
    // another branch is told who holds it, and --force takes it over
    git(&d, &["switch", "-q", "-c", "feat/two"]);
    let (ok, _, err) = fael(&d, &["claim", new]);
    assert!(
        !ok && err.contains("held @feat/one") && err.contains("--force"),
        "{err}"
    );
    let (ok, _, err) = fael(&d, &["claim", new, "--force"]);
    assert!(ok && err.contains("was held @feat/one"), "{err}");
    let (_, out, _) = fael(&d, &["find", "--kind", "issue"]);
    assert!(out.contains("(held @feat/two)"), "{out}");
    // a decision is no work item
    let (ok, out, _) = fael(
        &d,
        &[
            "add", "decision", "x", "--files", "src/a.rs", "--key", "a:b", "--json",
        ],
    );
    assert!(ok);
    let row: serde_json::Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    let (ok, _, err) = fael(&d, &["claim", row["id"].as_str().unwrap()]);
    assert!(!ok && err.contains("claim takes an open issue"), "{err}");
}

#[test]
fn a_hold_whose_branch_is_gone_is_taken_over_without_force() {
    let d = repo();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    git(&d, &["switch", "-q", "-c", "feat/dead"]);
    let (id, _) = issue(&d, "stale hold", "src/a.rs", &[]);
    assert!(fael(&d, &["claim", &id]).0);
    git(&d, &["switch", "-q", "feat/one"]);
    git(&d, &["branch", "-q", "-D", "feat/dead"]);
    // a claim keeps the id: the same id takes the hold over
    let (ok, _, err) = fael(&d, &["claim", &id]);
    assert!(
        ok && err.contains("was held @feat/dead (branch gone)"),
        "{err}"
    );
}

/// A hold that arrived by `fael sync` names a branch this clone only knows as
/// `origin/<branch>`: it is alive, so a plain claim is refused.
#[test]
fn a_hold_on_a_remote_tracking_branch_still_gates_the_claim() {
    let d = repo();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    git(&d, &["switch", "-q", "-c", "feat/far"]);
    let (id, _) = issue(&d, "held elsewhere", "src/a.rs", &[]);
    assert!(fael(&d, &["claim", &id]).0);
    git(&d, &["update-ref", "refs/remotes/origin/feat/far", "HEAD"]);
    git(&d, &["switch", "-q", "feat/one"]);
    git(&d, &["branch", "-q", "-D", "feat/far"]);
    let (ok, _, err) = fael(&d, &["claim", &id]);
    assert!(!ok && err.contains("held @feat/far"), "{err}");
}

/// Two worktrees, one clone, one issue: exactly one claimer wins.
#[test]
fn two_agents_racing_for_one_issue_have_one_winner() {
    let d = repo();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    let (id, _) = issue(&d, "contested", "src/a.rs", &[]);
    let w = d.with_file_name(format!("{}-w2", d.file_name().unwrap().to_string_lossy()));
    git(
        &d,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/two",
            w.to_str().unwrap(),
        ],
    );
    let wins: Vec<bool> = std::thread::scope(|s| {
        let a = s.spawn(|| fael(&d, &["claim", &id]).0);
        let b = s.spawn(|| fael(&w, &["claim", &id]).0);
        vec![a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(wins.iter().filter(|w| **w).count(), 1, "{wins:?}");
}

#[test]
fn next_claims_the_best_free_issue_and_prints_it() {
    let d = repo();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    issue(&d, "older plain", "src/a.rs", &[]);
    issue(&d, "urgent one", "src/b.rs", &["--urgent"]);
    issue(&d, "gated", "src/c.rs", &["--revisit", "vendor fixes 5xx"]);
    issue(&d, "theirs", "src/a.rs", &["--to", "someone-else"]);
    let (ok, out, err) = fael(&d, &["next"]);
    assert!(ok, "{err}");
    // urgent first; its text is printed so the agent starts without a second call
    assert!(out.lines().nth(1) == Some("urgent one"), "{out}");
    let (_, list, _) = fael(&d, &["find", "--kind", "issue"]);
    assert!(
        list.contains("urgent one (urgent 1, held @feat/one)"),
        "{list}"
    );
    // held by this branch now: the next call moves on; waiting and routed stay out
    let (ok, out, _) = fael(&d, &["next"]);
    assert!(ok && out.contains("older plain"), "{out}");
    let (ok, _, err) = fael(&d, &["next"]);
    assert!(!ok && err.contains("no ready issue"), "{err}");
}

#[test]
fn groups_flag_names_its_replacement() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["find", "--kind", "issue", "--groups"]);
    assert!(!ok && err.contains("fael find --kind issue"), "{err}");
}

/// An empty issue list says why like the flat one: closed rows need --all.
#[test]
fn an_empty_issue_list_says_all_adds_closed() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue"]);
    assert!(ok && out.is_empty(), "{out}");
    assert!(
        err.contains("no rows match kind=issue (open rows only; --all adds closed)"),
        "{err}"
    );
}

/// Only-closed issues answer in the same call: the closed rows --all would
/// match, marked, under the no-open-row header — no second round.
#[test]
fn only_closed_issue_list_shows_marked_rows() {
    let d = repo();
    let (id, _) = issue(&d, "ocr timeout", "src/a.rs", &[]);
    let (ok, _, err) = fael(&d, &["close", &id, "fixed"]);
    assert!(ok, "{err}");
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue"]);
    assert!(ok, "{err}");
    assert!(
        out.starts_with("no open row matches — 1 closed or superseded:")
            && out.contains("(closed)")
            && out.contains("ocr timeout"),
        "{out}"
    );
}

#[test]
fn issue_list_marks_rows_on_gone_files() {
    let d = repo();
    issue(&d, "ocr timeout", "src/a.rs", &[]);
    issue(&d, "auth typo", "src/c.rs", &[]);
    std::fs::remove_file(d.join("src/c.rs")).unwrap();
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue"]);
    assert!(ok, "{err}");
    let gone = out
        .lines()
        .find(|l| l.contains("auth typo"))
        .unwrap()
        .to_string();
    assert!(gone.contains("[Gone]"), "{out}");
    let live = out
        .lines()
        .find(|l| l.contains("ocr timeout"))
        .unwrap()
        .to_string();
    assert!(!live.contains("[Gone]"), "{out}");
}

#[test]
fn issue_list_marks_branch_rows_whose_branch_landed() {
    let d = repo();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    git(&d, &["switch", "-q", "-c", "feat/x"]);
    issue(&d, "side branch leak", "src/a.rs", &[]);
    std::fs::write(d.join("src/a.rs"), "// touched\n").unwrap();
    // only the touched file: `add -A` would swallow `state/` (FAEL_STATE_DIR
    // lives inside the test repo) and the squash merge would conflict on it
    git(&d, &["add", "src/a.rs"]);
    git(&d, &["commit", "-q", "-m", "work"]);
    git(&d, &["switch", "-q", "feat/one"]);
    // before the landing: tagged with the branch, no merge mark
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue"]);
    assert!(ok, "{err}");
    let line = out
        .lines()
        .find(|l| l.contains("side branch leak"))
        .unwrap()
        .to_string();
    assert!(line.ends_with("@feat/x"), "{out}");
    git(&d, &["merge", "-q", "--squash", "feat/x"]);
    git(&d, &["commit", "-q", "-m", "squash"]);
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue"]);
    assert!(ok, "{err}");
    let line = out
        .lines()
        .find(|l| l.contains("side branch leak"))
        .unwrap()
        .to_string();
    // the work touched the issue's file after filing: both tags hold
    assert!(
        line.ends_with("@feat/x (merged) (files changed since)"),
        "{out}"
    );
}

#[test]
fn issue_json_stays_flat() {
    let d = repo();
    issue(&d, "ocr timeout", "src/a.rs,src/b.rs", &[]);
    issue(&d, "scope leak", "src/b.rs", &[]);
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue", "--json"]);
    assert!(ok, "{err}");
    assert!(!out.contains("## group"), "{out}");
    let rows: Vec<serde_json::Value> = out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 2, "{out}");
    assert!(rows.iter().all(|v| v["kind"] == "issue"), "{out}");
}

#[test]
fn full_cut_line_asks_for_the_rest_in_one_call() {
    let d = repo();
    for i in 0..5 {
        issue(
            &d,
            &format!("full row {i} {}", "filler ".repeat(130)),
            "src/a.rs",
            &[],
        );
    }
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue", "--full"]);
    assert!(ok, "{err}");
    let shown = out.lines().filter(|l| l.starts_with("- [")).count();
    let want = format!(
        "next: fael find --kind issue --full --offset {shown} --limit {}\n",
        5 - shown
    );
    assert!(out.contains(&want), "{out}");
}
