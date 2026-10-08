//! PLAN-fael-experience-loop chunk 4: `fael stats` reads the default
//! branch's `fix:` commits and links them to fael — here by the id the
//! commit body names; the close names a check.

use super::{fael, json, repo};
use std::path::Path;
use std::process::Command;

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(d)
        // after the first usage line, whatever the clock's second
        .env("GIT_COMMITTER_DATE", "2099-01-01T00:00:00Z")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn stats_links_a_fix_commit_by_the_id_it_names() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "retry loops",
            "--files",
            "src/a.rs",
            "--key",
            "a:retry",
        ],
        "",
    );
    assert!(ok, "{err}");
    let id = &out[..8];
    // a push on the file: the repo's first usage line
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    assert!(fael(&d, &["hook", "read"], &input).0);
    let why = "`a.rs` retried forever → cap at 3; guard `tests/retry.rs`";
    assert!(fael(&d, &["close", "--key", "a:retry", why], "").0);
    git(&d, &["add", "src/a.rs"]);
    let body = format!("fix: cap retries\n\n(fael:{id})");
    git(&d, &["commit", "-q", "-m", &body]);
    git(
        &d,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "fix: nobody filed this",
        ],
    );
    git(
        &d,
        &["commit", "-q", "--allow-empty", "-m", "feat: not a fix"],
    );
    git(&d, &["branch", "-M", "main"]);
    let (ok, out, err) = fael(&d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let e = &v["experience"];
    assert_eq!(
        (&e["fix_commits"], &e["fix_commits_linked"]),
        (&2.into(), &1.into()),
        "{e}"
    );
    assert_eq!(
        (&e["fixed"], &e["closed_with_check"]),
        (&1.into(), &1.into()),
        "{e}"
    );
}
