//! Working the issue list in the real binary: waiting issues list last,
//! `claim` shows who holds one, `--groups` says what to fix in one PR, and
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
    let mut args = vec!["add", "issue", text, "--files", files, "--json"];
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
    let lines: Vec<&str> = out.lines().filter(|l| l.starts_with("- [")).collect();
    // newest first among the ready ones; the waiting one last, tagged
    assert!(
        lines[2].contains("ocr gated (waiting: vendor fixes 5xx)"),
        "{out}"
    );
    assert!(lines[0].contains("scope bug"), "{out}");
    assert!(
        out.contains("--groups") && out.contains("fael claim"),
        "{out}"
    );
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
fn groups_by_shared_files() {
    let d = repo();
    issue(&d, "ocr timeout", "src/a.rs,src/b.rs", &[]);
    issue(&d, "scope leak", "src/b.rs", &[]);
    issue(&d, "auth typo", "src/c.rs", &[]);
    let (ok, out, err) = fael(&d, &["find", "--kind", "issue", "--groups"]);
    assert!(ok, "{err}");
    assert!(
        out.starts_with("## group 1 · 2 rows · shared: src/b.rs\n"),
        "{out}"
    );
    let g2 = out.split("## group 2").nth(1).unwrap();
    assert!(
        g2.contains("auth typo") && !g2.contains("scope leak"),
        "{out}"
    );
    let (ok, _, err) = fael(&d, &["find", "--kind", "issue", "--groups", "--limit", "2"]);
    assert!(!ok && err.contains("--groups lists every match"), "{err}");
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

/// The hold outlived its branch (merged and deleted, or dropped) while the
/// issue stayed open: the holder's session start says so, once, with the
/// close command — nobody else's does.
#[test]
fn session_start_names_my_open_hold_whose_branch_is_gone() {
    use std::io::Write;
    let d = repo();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    let start = |email: &str| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
            .args(["hook", "session-start", "--client", "claude"])
            .current_dir(&d)
            .env("FAEL_STATE_DIR", d.join("state"))
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "user.email")
            .env("GIT_CONFIG_VALUE_0", email)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let input = serde_json::json!({ "cwd": d }).to_string();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        String::from_utf8_lossy(&c.wait_with_output().unwrap().stdout).into_owned()
    };
    git(&d, &["switch", "-q", "-c", "feat/done"]);
    let (id, _) = issue(&d, "ship the widget", "src/a.rs", &[]);
    assert!(fael(&d, &["claim", &id]).0);
    // the branch still exists: nothing to say
    assert!(!start("t@example.com").contains("branch gone"));
    git(&d, &["switch", "-q", "feat/one"]);
    git(&d, &["branch", "-q", "-D", "feat/done"]);
    let out = start("t@example.com");
    assert!(
        out.contains("held @feat/done — branch gone, issue still open")
            && out.contains("fael close "),
        "{out}"
    );
    // another reader did not hold it
    assert!(!start("other@example.com").contains("branch gone"));
    // closed: the line goes
    let (_, list, _) = fael(&d, &["find", "--kind", "issue"]);
    let cur = list.split("- [").nth(1).unwrap().split(']').next().unwrap();
    assert!(fael(&d, &["close", cur, "shipped in #1"]).0);
    assert!(!start("t@example.com").contains("branch gone"));
}
