//! `doctor` says what is actionable: flags it never reads are rejected (issue
//! 01M7T777), the header counts what to decide and what `--fix` clears with
//! info last, a missing file names the git fact behind it, and a list cut at
//! five ends with the command that prints the rest.

use super::{fael, repo};
use std::path::Path;
use std::process::Command;

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(d)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn add_issue(d: &Path, text: &str, files: &str) -> String {
    let (ok, out, err) = fael(d, &["add", "issue", text, "--files", files, "--force"]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

#[test]
fn doctor_rejects_a_flag_it_never_reads_and_writes_nothing() {
    let d = repo();
    let id = add_issue(&d, "on a gone file", "src/deleted.rs");
    for flags in [&["--fix", "--dry-run"][..], &["--force"]] {
        let mut args = vec!["doctor"];
        args.extend(flags);
        let (ok, out, err) = fael(&d, &args);
        assert!(!ok && out.is_empty(), "{args:?}: {out}");
        assert!(
            err.contains("doctor takes no --") && err.contains("--fix") && err.contains("--json"),
            "{err}"
        );
    }
    // `--yes` and `--fat` are retired step-2: gone from the parser entirely
    for flags in [&["--yes"][..], &["--json", "--fat"]] {
        let mut args = vec!["doctor"];
        args.extend(flags);
        let (ok, out, err) = fael(&d, &args);
        assert!(!ok && out.is_empty(), "{args:?}: {out}");
        assert!(err.contains("unknown flag"), "{err}");
    }
    // `--fix --dry-run` once ran the real fix: the Gone row must still be open
    let (_, out, _) = fael(&d, &["find", &id]);
    assert!(!out.contains("closed"), "{out}");
}

#[test]
fn the_header_counts_decisions_and_fixes_and_info_comes_last() {
    let d = repo();
    add_issue(&d, "on a gone file", "src/deleted.rs");
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    add_issue(&d, "half gone", "src/a.rs,src/removed.rs");
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0], "fael doctor: 1 need a decision · 1 --fix clears now · 2 info",
        "{out}"
    );
    let tags: Vec<String> = lines[1..]
        .iter()
        .map(|l| l.split(": ").next().unwrap().to_string())
        .collect();
    assert_eq!(
        tags,
        [
            "note [PartGone]",
            "note [Gone] [--fix]",
            "info [Local]",
            "info [GhOffline]"
        ],
        "{out}"
    );
    // `--json` keeps its shape: one object per problem, same keys, info still listed
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert!(ps.iter().any(|p| p["kind"] == "local"), "{out}");
    for p in &ps {
        let mut keys: Vec<&str> = p.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["detail", "fixable", "ids", "kind", "severity"],
            "{p}"
        );
    }
}

#[test]
fn a_missing_file_names_the_commit_that_removed_it() {
    let d = repo();
    for f in ["src/gone.rs", "src/kept.rs"] {
        std::fs::write(d.join(f), "x\n").unwrap();
    }
    git(&d, &["add", "."]);
    git(
        &d,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "add both"],
    );
    git(&d, &["rm", "-q", "src/gone.rs"]);
    git(
        &d,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "drop the gone tool",
        ],
    );
    std::fs::remove_file(d.join("src/kept.rs")).unwrap(); // deleted, never committed
    add_issue(&d, "on the removed file", "src/gone.rs");
    add_issue(&d, "on a typo", "src/never.rs");
    add_issue(&d, "on a dirty delete", "src/kept.rs");
    let (ok, out, err) = fael(&d, &["doctor"]);
    assert!(ok, "{out} {err}");
    assert!(
        out.contains("(src/gone.rs — removed in ") && out.contains("\"drop the gone tool\")"),
        "{out}"
    );
    assert!(out.contains("(src/never.rs — never in git)"), "{out}");
    assert!(
        out.contains("(src/kept.rs — deleted, not committed)"),
        "{out}"
    );
}

#[test]
fn a_list_cut_at_five_ends_with_the_command_for_the_rest() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    for i in 0..8 {
        add_issue(&d, &format!("part {i}"), &format!("src/a.rs,src/m{i}.rs"));
    }
    let (_, out, _) = fael(&d, &["doctor"]);
    let line = out.lines().find(|l| l.contains("[PartGone]")).unwrap();
    assert!(line.ends_with("· +3 more: `fael doctor --json`"), "{line}");
    // the command it names prints every id and every entry
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    let p = ps.iter().find(|p| p["kind"] == "partgone").unwrap();
    assert_eq!(p["ids"].as_array().unwrap().len(), 8, "{p}");
    let detail = p["detail"].as_str().unwrap();
    for i in 0..8 {
        assert!(detail.contains(&format!("src/m{i}.rs")), "m{i}: {detail}");
    }
    // a list that fits in five carries no hint
    let (_, out, _) = fael(
        &d,
        &["add", "issue", "gone", "--files", "src/x.rs", "--force"],
    );
    assert!(!out.is_empty());
    let (_, out, _) = fael(&d, &["doctor"]);
    let gone = out.lines().find(|l| l.contains("[Gone]")).unwrap();
    assert!(!gone.contains("more:"), "{gone}");
}
