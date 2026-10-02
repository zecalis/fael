//! `doctor` for log health: union setup, gone/part-gone notes, stale
//! backtick pointers, phantom id citations, and quarantine of broken lines.

use super::{fael, repo};
use std::path::PathBuf;

/// `add` a row of `kind` on its own file; returns the new id (stdout's first token).
fn add(d: &std::path::Path, kind: &str, name: &str, text: &str) -> String {
    std::fs::write(d.join(name), "").unwrap();
    let (ok, out, err) = fael(d, &["add", kind, text, "--files", name]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// Flip the id's last char to another valid Crockford char: an id-shaped
/// token no row owns, so it is `Missing`, never `Many`.
fn phantom_of(id: &str) -> String {
    let mut f = id.to_string();
    let last = if f.ends_with('A') { 'B' } else { 'A' };
    f.pop();
    f.push(last);
    assert!(fael_core::looks_like_id(&f), "{f}");
    f
}

fn git(d: &std::path::Path, args: &[&str]) {
    assert!(
        std::process::Command::new("git")
            .args(args)
            .current_dir(d)
            .status()
            .unwrap()
            .success(),
        "git {args:?}"
    );
}

#[test]
fn doctor_fails_without_union_then_fix_repairs() {
    let d = repo();
    // no log yet: info only, exit 0
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok && out.contains("1 problem(s)"), "{out}");
    // adopt fael without the union line: error, exit 1
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, _, _) = fael(&d, &["add", "note", "first row", "--files", "src/a.rs"]);
    assert!(ok);
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(!ok && out.contains("error"), "{out}");
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    assert!(out.contains("merge=union"), "{out}");
    assert!(d.join(".gitattributes").exists());
    // union repaired; the one note left is the uncommitted row (tracked)
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(
        ok && out.contains("1 problem(s) (0 error(s), 1 note(s))")
            && out.contains("note [Late]: 1 .fael/log file(s) uncommitted"),
        "{out}"
    );
    // excluded locally on purpose: a note, not an error; in .gitignore: an error
    std::fs::write(d.join(".git/info/exclude"), ".fael/log/\n").unwrap();
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok && out.contains("note [Ignored]"), "{out}");
    std::fs::write(d.join(".gitignore"), ".fael/log/\n").unwrap();
    std::fs::write(d.join(".git/info/exclude"), "").unwrap();
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(!ok && out.contains("error [Ignored]"), "{out}");
    // an open row on a deleted file is reported, never an error
    fael(
        &d,
        &[
            "add",
            "issue",
            "on a gone file",
            "--files",
            "src/deleted.rs",
        ],
    );
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Gone]: 1 open row(s)") && out.contains("src/deleted.rs"),
        "{out}"
    );
    // one file left, one gone: still pushes, reported apart, naming only the gone file
    fael(
        &d,
        &[
            "add",
            "issue",
            "half gone",
            "--files",
            "src/a.rs,src/removed.rs",
            "--force",
        ],
    );
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Gone]: 1 open row(s)")
            && out.contains("note [PartGone]: 1 open row(s)")
            && out.contains("→ src/removed.rs"),
        "{out}"
    );
}

#[test]
fn doctor_flags_stale_backtick_paths() {
    let d = repo();
    std::fs::write(d.join("keep.yaml"), "").unwrap();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    // the row files a live file; the backticked pointer is prose, not files[]
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "catalog ใน `keep.yaml`",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Stale]"), "{out}");
    // the file goes away, the pointer stays: Stale names the row id
    std::fs::remove_file(d.join("keep.yaml")).unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Stale]: 1 open row(s)") && out.contains("→ keep.yaml"),
        "{out}"
    );
    // talk in backticks (no path) never flags
    fael(
        &d,
        &["add", "note", "run `merge=union` after", "--files", "src"],
    );
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Stale]: 1 open row(s)") && !out.contains("merge=union"),
        "{out}"
    );
}

#[test]
fn doctor_flags_open_row_citing_fake_id() {
    let d = repo();
    let id = add(&d, "note", "src/a.rs", "keeper row");
    let fake = phantom_of(&id);
    let citer = add(&d, "note", "src/b.rs", &format!("see {fake} for context"));
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    // info only: exit stays 0, but the dead citation is named
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Phantom]: 1 reference(s)") && out.contains(&fake),
        "{out}"
    );
    assert!(out.contains("compact --prune"), "{out}");
    // `--json` carries the citing row's full id under a lowercase label
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps: Vec<serde_json::Value> = serde_json::from_str(out.trim()).unwrap();
    let ph = ps.iter().find(|p| p["kind"] == "phantom").expect("phantom");
    assert_eq!(ph["severity"], serde_json::json!("info"));
    assert_eq!(ph["fixable"], serde_json::json!(false));
    assert!(ph["detail"].as_str().unwrap().contains(&fake), "{ph}");
    assert_eq!(ph["ids"], serde_json::json!([citer]), "{ph}");
}

#[test]
fn doctor_flags_fake_id_in_a_row_title() {
    let d = repo();
    let seed = add(&d, "note", "src/a.rs", "keeper row");
    let fake = phantom_of(&seed);
    std::fs::write(d.join("src/b.rs"), "").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "no id in the body",
            "--title",
            &format!("fixed by {fake}"),
            "--files",
            "src/b.rs",
        ],
    );
    assert!(ok, "{err}");
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    // a citation in the title is as visible as one in the text
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Phantom]: 1 reference(s)") && out.contains(&fake),
        "{out}"
    );
}

#[test]
fn doctor_flags_close_reason_citing_fake_id() {
    let d = repo();
    let seed = add(&d, "note", "src/a.rs", "keeper row");
    let fake = phantom_of(&seed);
    let target = add(&d, "issue", "src/b.rs", "broken thing");
    let (ok, _, err) = fael(&d, &["close", &target, &format!("fixed, see {fake}")]);
    assert!(ok, "{err}");
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    // the fake lives in close text, not in any open row — still Phantom
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Phantom]: 1 reference(s)") && out.contains(&fake),
        "{out}"
    );
}

#[test]
fn doctor_flags_phantom_id_in_markdown() {
    let d = repo();
    let seed = add(&d, "note", "src/a.rs", "keeper row");
    let fake = phantom_of(&seed);
    // a dead citation written in a plan file — named with the line it sits on
    std::fs::write(
        d.join("PLAN-x.md"),
        format!("# x\n\nsee {fake} for the steps\n"),
    )
    .unwrap();
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Phantom]: 1 reference(s) to ids with no row in markdown")
            && out.contains(&format!("PLAN-x.md:3 → {fake}"))
            // an id from another repo's log reads as dead here: cite the key
            && out.contains("cite its key instead"),
        "{out}"
    );
    // no row owns a doc citation: --json carries an empty ids list
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps: Vec<serde_json::Value> = serde_json::from_str(out.trim()).unwrap();
    let ph = ps.iter().find(|p| p["kind"] == "phantom").expect("phantom");
    assert_eq!(ph["ids"], serde_json::json!([]), "{ph}");
    // the same id inside a fenced block is an example, never a citation
    std::fs::write(
        d.join("PLAN-x.md"),
        format!("# x\n\n```json\n{{\"id\":\"{fake}\"}}\n```\n"),
    )
    .unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Phantom]"), "{out}");
    // and a live id in prose stays quiet
    std::fs::write(d.join("PLAN-x.md"), format!("# x\n\nsee {seed}\n")).unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Phantom]"), "{out}");
}

#[test]
fn doctor_stays_clean_for_real_ambiguous_and_closed_citations() {
    let d = repo();
    let keeper = add(&d, "note", "src/a.rs", "keeper row");
    let fake = phantom_of(&keeper);
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    // a real id resolves: silent
    add(&d, "note", "src/b.rs", &format!("follows up {keeper}"));
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Phantom]"), "{out}");
    // an abbreviation that decayed as the log grew exists: silent, never missing
    let mut ids: Vec<String> = vec![];
    let mut prefix = String::new();
    for i in 0..10 {
        ids.push(add(
            &d,
            "note",
            &format!("src/f{i}.rs"),
            &format!("row {i}"),
        ));
        if let Some(other) = ids[..ids.len() - 1]
            .iter()
            .find(|o| o[..8] == ids[ids.len() - 1][..8])
        {
            prefix = other[..8].to_string();
            break;
        }
    }
    assert!(!prefix.is_empty(), "no shared 8-char prefix in {ids:?}");
    add(&d, "note", "src/g.rs", &format!("cites {prefix} here"));
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Phantom]"), "{out}");
    // a closed row's own text went quiet with the row: only close texts
    // and open rows are scanned, so closing clears the citation
    let target = add(&d, "issue", "src/h.rs", &format!("broken, see {fake}"));
    let (ok, _, err) = fael(&d, &["close", &target, "really fixed"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Phantom]"), "{out}");
}

#[test]
fn doctor_ignores_id_only_on_another_clone_branch() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "row on main", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "rows"]);

    // another clone: its own journal, so the union here never sees its rows
    let other = d.join("other");
    git(
        &d,
        &["clone", "-q", d.to_str().unwrap(), other.to_str().unwrap()],
    );
    git(&other, &["config", "user.name", "Other Clone"]);
    git(&other, &["config", "user.email", "o@example.com"]);
    git(&other, &["checkout", "-qb", "feat/y"]);
    std::fs::write(other.join("src/c.rs"), "// c\n").unwrap();
    let (ok, out, err) = fael(
        &other,
        &["add", "note", "row C on feat", "--files", "src/c.rs"],
    );
    assert!(ok, "{err}");
    let cid = out.split_whitespace().next().unwrap().to_string();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-qm", "rows C"]);
    git(&other, &["push", "-q", "origin", "feat/y"]);

    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    // the union has no such row — only the branch escalation clears it
    add(&d, "note", "src/d.rs", &format!("ref {cid} for context"));
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Phantom]"), "{out}");
}

#[test]
fn doctor_json_lists_full_row_ids() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "on a gone file",
            "--files",
            "src/deleted.rs",
        ],
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    // `--json` carries the full id (not just the abbreviated example in detail)
    // (exit may be 1 — no union line yet is an unrelated error)
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps: Vec<serde_json::Value> = serde_json::from_str(out.trim()).unwrap();
    let gone = ps.iter().find(|p| p["kind"] == "gone").expect("gone");
    assert_eq!(gone["ids"], serde_json::json!([id]), "{gone}");
    // the id is actionable: closing it clears the problem
    let (ok, _, err) = fael(&d, &["close", &id, "moved"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    assert!(!out.contains("\"gone\""), "{out}");
}

#[test]
fn doctor_quarantines_a_broken_line() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "decision", "keep me", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let wdir = std::fs::read_dir(d.join(".fael/log"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let log: Vec<PathBuf> = std::fs::read_dir(&wdir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(log.len(), 1);
    let mut s = std::fs::read_to_string(&log[0]).unwrap();
    s.push_str("not json\n");
    std::fs::write(&log[0], s).unwrap();
    let (ok, _, _) = fael(&d, &["doctor"]);
    assert!(!ok);
    let (ok, out, _) = fael(&d, &["doctor", "--fix", "--json"]);
    assert!(ok, "{out}");
    let q: Vec<_> = std::fs::read_dir(d.join(".fael/quarantine"))
        .unwrap()
        .collect();
    assert_eq!(q.len(), 1);
    let (_, out, _) = fael(&d, &["find", "--all"]);
    assert!(out.contains("keep me"), "{out}");
}
