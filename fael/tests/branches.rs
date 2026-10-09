//! `find --branches` (row-hygiene chunk 9) through the real binary: rows on
//! an unmerged branch read without a checkout, tagged ` @<branch>`, and
//! untagged-once after the merge. Since the journal (chunk 1) the plain
//! `find` already sees the clone's journal rows tagged — `--branches` only
//! adds rows the journal never saw (another clone's branch, pre-journal rows).

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(d: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(d)
            .status()
            .unwrap()
            .success(),
        "git {args:?}"
    );
}

fn git_out(d: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        // never the developer's real usage log or session (fael:01M4F3G0)
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join(format!("fael-test-state-{}", std::process::id())),
        )
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-branches-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q"]);
    git(&d, &["config", "user.name", "Branch Test"]);
    git(&d, &["config", "user.email", "branch@example.com"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

/// `main` holds row B, `feat/x` (unmerged) holds row A — both committed, so
/// the branch tips carry their rows. Returns the main branch name.
fn main_and_feat(d: &Path) -> String {
    let main = git_out(d, &["symbolic-ref", "--short", "HEAD"]);
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, _, err) = fael(d, &["add", "note", "row B on main", "--files", "src/b.rs"]);
    assert!(ok, "{err}");
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "rows B"]);
    git(d, &["checkout", "-qb", "feat/x"]);
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(d, &["add", "note", "row A on feat", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "rows A"]);
    git(d, &["checkout", "-q", &main]);
    main
}

#[test]
fn find_branches_tags_unmerged_rows_and_plain_find_tags_journal_rows() {
    let d = repo();
    let main = main_and_feat(&d);

    // plain find: the union read sees the clone's journal rows too — the
    // foreign row lists tagged with its stamped branch, the local one untagged
    let (ok, out, _) = fael(&d, &["find"]);
    assert!(ok, "{out}");
    assert!(out.contains("row B on main"), "{out}");
    assert!(out.contains("row A on feat"), "{out}");
    let a_line = out.lines().find(|l| l.contains("row A on feat")).unwrap();
    assert!(a_line.ends_with("@feat/x"), "{a_line}");
    let b_line = out.lines().find(|l| l.contains("row B on main")).unwrap();
    assert!(!b_line.contains('@'), "{b_line}");

    // --branches: both, the foreign row tagged once, the local row untagged
    let (ok, out, _) = fael(&d, &["find", "--branches"]);
    assert!(ok, "{out}");
    assert!(out.contains("row B on main"), "{out}");
    assert!(out.contains("row A on feat"), "{out}");
    assert_eq!(out.matches("row A on feat").count(), 1, "{out}");
    assert_eq!(out.matches("row B on main").count(), 1, "{out}");
    let a_line = out.lines().find(|l| l.contains("row A on feat")).unwrap();
    assert!(a_line.ends_with("@feat/x"), "{a_line}");
    let b_line = out.lines().find(|l| l.contains("row B on main")).unwrap();
    assert!(!b_line.contains('@'), "{b_line}");

    // `find <id> --branches` tags the foreign row too, not only a list
    let id = a_line
        .strip_prefix("- [")
        .and_then(|l| l.split(']').next())
        .unwrap();
    let (ok, out, _) = fael(&d, &["find", id, "--branches"]);
    assert!(
        ok && out.contains("row A on feat") && out.contains("@feat/x"),
        "{out}"
    );

    // the read moved nothing: same branch, and nothing outside .fael/
    // touched (find refreshes its rename cache inside .fael/ — that churn
    // predates --branches and is not a checkout)
    assert_eq!(git_out(&d, &["symbolic-ref", "--short", "HEAD"]), main);
    let dirty = git_out(&d, &["status", "--porcelain", "--untracked-files=no"]);
    assert!(dirty.lines().all(|l| l.contains(".fael/")), "{dirty}");

    // kickoff --branches sees the foreign row too (its file must exist —
    // kickoff drops rows whose files are all gone)
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, _) = fael(&d, &["kickoff", "--branches"]);
    assert!(ok, "{out}");
    assert!(out.contains("row A on feat"), "{out}");
    let a_line = out.lines().find(|l| l.contains("row A on feat")).unwrap();
    assert!(a_line.ends_with("@feat/x"), "{a_line}");
}

#[test]
fn find_branches_after_merge_shows_each_row_once_untagged() {
    let d = repo();
    main_and_feat(&d);
    git(&d, &["add", "-A"]);
    git(&d, &["merge", "-q", "--no-edit", "feat/x"]);

    // merged: the row is in HEAD now — listed once, with no branch tag, and
    // the merged branch no longer counts as a foreign source
    for args in [&["find"][..], &["find", "--branches"]] {
        let (ok, out, _) = fael(&d, args);
        assert!(ok, "{out:?}");
        assert!(out.contains("row A on feat"), "{out}");
        assert_eq!(out.matches("row A on feat").count(), 1, "{out}");
        assert!(!out.contains("@feat/x"), "{out}");
    }
}

/// One MCP `find` call through `fael mcp` — the raw JSON-RPC reply.
fn mcp_find(d: &Path, args: serde_json::Value) -> String {
    use std::io::Write;
    use std::process::Stdio;
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        // never the developer's real usage log or session (fael:01M4F3G0)
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join(format!("fael-test-state-{}", std::process::id())),
        )
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .arg("mcp")
        .current_dir(d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "find", "arguments": args}});
    c.stdin
        .take()
        .unwrap()
        .write_all((call.to_string() + "\n").as_bytes())
        .unwrap();
    String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap()
}

#[test]
fn mcp_find_branches_tags_like_cli() {
    let d = repo();
    main_and_feat(&d);
    let out = mcp_find(&d, serde_json::json!({"branches": true}));
    assert!(out.contains("row A on feat"), "{out}");
    assert!(out.contains("@feat/x"), "{out}");
    assert!(out.contains("row B on main"), "{out}");
}

/// Chunk 4 S4: when `--branches` adds no row the union read lacks, it says
/// why — `local` (rows live in the journal) or tracked (the journal already
/// has them, or the log is gitignored) — on stderr, and over MCP as a trailing
/// line. Rows really added (another clone's branch) print no note.
#[test]
fn find_branches_says_why_when_it_adds_nothing() {
    let d = repo();
    std::fs::write(d.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
    main_and_feat(&d);
    let (ok, plain, _) = fael(&d, &["find", "row"]);
    assert!(
        ok && plain.contains("row A on feat") && plain.contains("@feat/x"),
        "{plain}"
    );
    let (ok, wide, err) = fael(&d, &["find", "row", "--branches"]);
    assert!(ok);
    assert_eq!(wide, plain);
    assert!(err.contains("store = \"local\""), "{err}");
    let out = mcp_find(&d, serde_json::json!({"branches": true}));
    assert!(out.contains("added no rows beyond plain find"), "{out}");
    let (_, _, err) = fael(&d, &["find", "row"]);
    assert!(err.is_empty(), "{err}");

    // tracked, one clone: the journal already holds feat/x's row
    let t = repo();
    main_and_feat(&t);
    let (ok, _, err) = fael(&t, &["find", "--branches"]);
    assert!(
        ok && err.contains("gitignored") && !err.contains("local"),
        "{err}"
    );

    // another clone's journal never saw feat/x: --branches adds it, no note
    let c = std::env::temp_dir().join(format!("fael-branches-{}", fael_core::ulid()));
    git(
        &t,
        &["clone", "-q", t.to_str().unwrap(), c.to_str().unwrap()],
    );
    let (ok, out, err) = fael(&c, &["find", "--branches"]);
    assert!(ok && out.contains("row A on feat"), "{out}");
    assert!(!err.contains("--branches"), "{err}");
}
