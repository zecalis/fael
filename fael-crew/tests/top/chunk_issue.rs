//! `fael chunk add --issue` through the real binary (PLAN-fael-board l2): only an open issue
//! links, the brief prints it with the close rule, and the board shows the fixing chunk —
//! under needs you `issue-open` once it is done while the issue stays open.

use serde_json::{Value, json};
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

/// The ledger id `fael add` prints first on stdout.
fn row(d: &Path, args: &[&str]) -> String {
    let (ok, out, err) = fael(d, args);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

fn board(d: &Path) -> Value {
    let (ok, out, err) = fael(d, &["board", "--json"]);
    assert!(ok, "{err}");
    serde_json::from_str(&out).unwrap()
}

fn issue_open(b: &Value) -> Vec<String> {
    b["needs_you"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "issue-open")
        .map(|n| n["uid"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_chunk_fixes_open_issues_and_the_board_follows_it() {
    let d = std::env::temp_dir().join(format!("fael-issue-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    let mut git = Command::new("git");
    assert!(
        git.args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let lag = row(
        &d,
        &[
            "add",
            "issue",
            "The count lags one refresh",
            "--files",
            "a.rs",
        ],
    );
    let old = row(&d, &["add", "issue", "Old bug", "--files", "a.rs"]);
    let pick = row(&d, &["add", "decision", "Use one query", "--files", "a.rs"]);
    assert!(
        fael(
            &d,
            &["close", &old, "old bug → fixed; guard `src/a_test.rs`"]
        )
        .0
    );
    let add = |ids: &str| fael(&d, &["chunk", "add", "fix", "--brief", "b", "--issue", ids]);

    // rejected, naming the id: unknown, closed, not an issue — and nothing is added
    for (bad, says) in [
        ("01ZZZZZZZZ", "no row with id"),
        (old.as_str(), "is closed"),
        (pick.as_str(), "not an issue"),
    ] {
        let (ok, _, err) = add(&format!("{lag},{bad}"));
        assert!(
            !ok && err.contains(says) && err.contains(bad),
            "{bad}: {err}"
        );
    }

    // a unique prefix links the full id
    let (ok, uid, err) = add(&lag[..12]);
    assert!(ok, "{err}");
    let uid = uid.trim();
    let (ok, brief, err) = fael(&d, &["chunk", "start", uid, "--run", "R", "--no-worktree"]);
    assert!(ok, "{err}");
    assert!(
        brief.contains(&format!("> {lag} — open — The count lags one refresh")),
        "{brief}"
    );
    assert!(brief.contains("<cause> → <fix>; guard \\`<test path>\\`\"` (done never"));

    let b = board(&d);
    let p = &b["projects"][0];
    assert_eq!(
        p["chunks"].as_array().unwrap().len(),
        1,
        "a rejected add added nothing"
    );
    assert_eq!(p["chunks"][0]["issues"], json!([lag]));
    assert_eq!(
        p["ledger"]["issues"][0]["chunk"],
        json!({"uid": uid, "state": "running"})
    );
    assert!(issue_open(&b).is_empty(), "running: not yet the owner's");

    // a re-filed issue (new id) keeps its chunk: the link follows the supersede chain
    let lag = row(
        &d,
        &[
            "add",
            "issue",
            "The count lags one refresh after a merge",
            "--files",
            "a.rs",
            "--supersedes",
            &lag,
        ],
    );
    let i = &board(&d)["projects"][0]["ledger"]["issues"][0];
    assert_eq!(i["id"], json!(lag));
    assert_eq!(i["chunk"], json!({"uid": uid, "state": "running"}));

    // done does not close the issue: the owner sees it until someone does
    let (ok, _, err) = fael(
        &d,
        &[
            "chunk", "done", uid, "fixed", "--left", "none", "--pr", "5", "--run", "R",
        ],
    );
    assert!(ok, "{err}");
    assert_eq!(issue_open(&board(&d)), [uid]);
    assert!(
        fael(
            &d,
            &[
                "close",
                &lag,
                "count lags → reads chunk list; guard `src/a_test.rs`"
            ]
        )
        .0
    );
    assert!(issue_open(&board(&d)).is_empty());
}

/// Linked issues print their live state at start (issues-start): open,
/// closed with its first close line, superseded with its successor, and a
/// purged id as not found — one line each in the `ledger issues` block.
#[test]
fn linked_issues_show_state_at_start() {
    let d = std::env::temp_dir().join(format!("fael-issue-state-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let open = row(&d, &["add", "issue", "open problem", "--files", "a.rs"]);
    let shut = row(&d, &["add", "issue", "was broken", "--files", "a.rs"]);
    let old = row(&d, &["add", "issue", "old way", "--files", "a.rs"]);
    let gone = row(&d, &["add", "issue", "gone row", "--files", "a.rs"]);
    // link all four while open — add rejects closed, so close/supersede after
    let (ok, uid, err) = fael(
        &d,
        &[
            "chunk",
            "add",
            "fix",
            "--brief",
            "b",
            "--issue",
            &format!("{open},{shut},{old},{gone}"),
        ],
    );
    assert!(ok, "{err}");
    let uid = uid.trim().to_string();
    assert!(
        fael(
            &d,
            &["close", &shut, "was broken → fixed; guard `src/a_test.rs`"]
        )
        .0
    );
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "new way",
            "--files",
            "a.rs",
            "--supersedes",
            &old,
        ],
    );
    assert!(ok, "{err}");
    let new = out.split_whitespace().next().unwrap().to_string();
    assert!(fael(&d, &["purge", &gone]).0);
    let (ok, brief, err) = fael(
        &d,
        &[
            "chunk",
            "start",
            &uid,
            "--run",
            "R",
            "--no-worktree",
            "--force",
            "--because",
            "test shows closed state",
        ],
    );
    assert!(ok, "{err}");
    assert!(
        brief.contains(&format!("> {open} — open — open problem")),
        "{brief}"
    );
    assert!(
        brief.contains(&format!("> {shut} — closed: was broken → fixed")),
        "{brief}"
    );
    assert!(
        brief.contains(&format!("> {old} — superseded → {new}")),
        "{brief}"
    );
    assert!(
        brief.contains(&format!("> {gone} — not found in the ledger")),
        "{brief}"
    );
}

/// `chunk edit --issue +<id>` (issues-start, issue 01M75C6A9GQZN93JMSZZ0S3MTR):
/// an in-scope link keeps `approved_at` exactly, an out-of-scope link clears
/// it and says so, a closed issue is rejected — exact field values asserted.
#[test]
fn chunk_edit_links_issues_with_scope_approval() {
    let d = std::env::temp_dir().join(format!("fael-issue-edit-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let (ok, uid, err) = fael(
        &d,
        &["chunk", "add", "fix", "--brief", "b", "--scope", "a.rs"],
    );
    assert!(ok, "{err}");
    let uid = uid.trim().to_string();
    assert!(
        fael(
            &d,
            &[
                "chunk", "approve", &uid, "--model", "opus", "--client", "opencode"
            ]
        )
        .0
    );
    let approval = |d: &Path| {
        let (_, out, _) = fael(d, &["chunk", "show", &uid, "--json"]);
        let v: Value = serde_json::from_str(&out).unwrap();
        (
            v["approval"]["at"].as_str().map(String::from),
            v["approval"]["model"].as_str().map(String::from),
            v["approval"]["client"].as_str().map(String::from),
        )
    };
    let before = approval(&d);
    assert!(before.0.is_some(), "approve sets approval: {before:?}");

    // in-scope: files all under the scope — approval kept exactly
    let inside = row(&d, &["add", "issue", "inside bug", "--files", "a.rs"]);
    let (ok, out, err) = fael(
        &d,
        &["chunk", "edit", &uid, "--issue", &format!("+{inside}")],
    );
    assert!(ok, "{err}");
    assert!(out.contains("edited issue"), "{out}");
    assert!(!out.contains("approval cleared"), "{out}");
    assert_eq!(approval(&d), before, "in-scope link keeps approval exactly");

    // out-of-scope: a file outside clears it and says so
    let outside = row(&d, &["add", "issue", "outside bug", "--files", "b.rs"]);
    let (ok, out, err) = fael(
        &d,
        &["chunk", "edit", &uid, "--issue", &format!("+{outside}")],
    );
    assert!(ok, "{err}");
    assert!(out.contains("approval cleared"), "{out}");
    let (at, model, client) = approval(&d);
    assert_eq!(
        (at, model, client),
        (None, None, None),
        "out-of-scope clears approval"
    );

    // closed: rejected, naming the id
    assert!(
        fael(
            &d,
            &[
                "chunk", "approve", &uid, "--model", "opus", "--client", "opencode"
            ]
        )
        .0
    );
    let shut = row(&d, &["add", "issue", "shut bug", "--files", "a.rs"]);
    assert!(
        fael(
            &d,
            &["close", &shut, "shut bug → fixed; guard `src/a_test.rs`"]
        )
        .0
    );
    let kept = approval(&d);
    let (ok, _, err) = fael(&d, &["chunk", "edit", &uid, "--issue", &format!("+{shut}")]);
    assert!(
        !ok && err.contains(&shut) && err.contains("is closed"),
        "{err}"
    );
    assert_eq!(approval(&d), kept, "a rejected link leaves approval alone");

    // the flag is `+<id>`: a bare id suggests the exact call
    let bare = row(&d, &["add", "issue", "bare bug", "--files", "a.rs"]);
    let (ok, _, err) = fael(&d, &["chunk", "edit", &uid, "--issue", &bare]);
    assert!(!ok && err.contains(&format!("--issue +{bare}")), "{err}");
}
