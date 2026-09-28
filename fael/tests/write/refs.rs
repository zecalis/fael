//! Write path (PLAN-fael-id-refs chunk-2): prose citing an id with no row
//! behind it gets one info line per id — the row is still written, never
//! rejected, and the line is never `warning:`-prefixed.

use super::{fael, repo, row_json};
use std::path::Path;
use std::process::Command;

const LINE: &str = "copy ids from fael find";

/// `add` a note on its own file; returns the new row's id (stdout's first token).
fn add(d: &Path, name: &str, text: &str) -> String {
    std::fs::write(d.join("src").join(name), "// x\n").unwrap();
    let (ok, out, err) = fael(
        d,
        &["add", "note", text, "--files", &format!("src/{name}")],
        "",
    );
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

#[test]
fn add_citing_fake_id_warns_but_writes() {
    let d = repo();
    let id = add(&d, "a.rs", "keeper row");
    let fake = phantom_of(&id);
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("see {fake} for context"),
            "--files",
            "src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(
        err.contains(&format!("no row with id {fake}"))
            && err.contains("cited in the text")
            && err.contains(LINE),
        "{err}"
    );
    assert!(!err.contains("warning:"), "{err}");
    // the row is filed regardless — the citation is findable text
    assert_eq!(
        row_json(&d, &fake)["text"].as_str().unwrap(),
        format!("see {fake} for context")
    );
}

#[test]
fn add_citing_real_id_stays_silent() {
    let d = repo();
    let id = add(&d, "a.rs", "keeper row");
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("follows up {id}"),
            "--files",
            "src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("no row with id"), "{err}");
}

#[test]
fn close_reason_citing_fake_id_warns_but_closes() {
    let d = repo();
    let seed = add(&d, "a.rs", "keeper row");
    let fake = phantom_of(&seed);
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "broken thing", "--files", "src/b.rs"],
        "",
    );
    assert!(ok, "{err}");
    let target = row_json(&d, "broken thing")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (ok, _, err) = fael(&d, &["close", &target, &format!("fixed, see {fake}")], "");
    assert!(ok, "{err}");
    assert!(
        err.contains(&format!("no row with id {fake}")) && err.contains(LINE),
        "{err}"
    );
    // the issue really closed — the close row points at the target
    let (_, out, _) = fael(&d, &["find", "--json", "--all"], "");
    assert!(
        out.lines()
            .any(|l| l.contains(&target) && l.contains("fixed, see")),
        "{out}"
    );
}

#[test]
fn close_reason_citing_the_target_stays_silent() {
    let d = repo();
    add(&d, "a.rs", "keeper row");
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "broken thing", "--files", "src/b.rs"],
        "",
    );
    assert!(ok, "{err}");
    let target = row_json(&d, "broken thing")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (ok, _, err) = fael(
        &d,
        &["close", &target, &format!("done, closing {target}")],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("no row with id"), "{err}");
}

#[test]
fn id_only_on_another_clones_branch_stays_silent() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "row on main", "--files", "src/a.rs"],
        "",
    );
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
        "",
    );
    assert!(ok, "{err}");
    let cid = out.split_whitespace().next().unwrap().to_string();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-qm", "rows C"]);
    git(&other, &["push", "-q", "origin", "feat/y"]);

    // the union has no such row — only the branch escalation clears it
    std::fs::write(d.join("src/d.rs"), "// d\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("ref {cid} for context"),
            "--files",
            "src/d.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("no row with id"), "{err}");
}

#[test]
fn ambiguous_prefix_stays_silent() {
    let d = repo();
    // ULID heads share ~256 ms per 8 chars — rapid adds collide on one
    let mut ids: Vec<String> = vec![];
    let mut prefix = String::new();
    for i in 0..10 {
        ids.push(add(&d, &format!("f{i}.rs"), &format!("row {i}")));
        if let Some(other) = ids[..ids.len() - 1]
            .iter()
            .find(|o| o[..8] == ids[ids.len() - 1][..8])
        {
            prefix = other[..8].to_string();
            break;
        }
    }
    assert!(!prefix.is_empty(), "no shared 8-char prefix in {ids:?}");
    // an abbreviation that decayed as the log grew exists — it is not a phantom
    std::fs::write(d.join("src/g.rs"), "// g\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("cites {prefix} here"),
            "--files",
            "src/g.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("no row with id"), "{err}");
}
