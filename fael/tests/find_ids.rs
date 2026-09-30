//! `find` with an id-shaped query (PLAN-fael-id-refs chunk-1): an id is an
//! id, never text. A real id shows the row (escalating to unmerged branches
//! on a union miss); a missing one rejects and names the rows that only
//! mention the string; an ambiguous prefix rejects; `--text` forces the
//! literal text search.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Per-child `FAEL_STATE_DIR` under the repo, so a real session on this
/// machine never leaks in (same shape as `cli.rs`).
fn state_env(c: &mut Command, dir: &Path) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    c.env("FAEL_STATE_DIR", root.join("state"));
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(dir);
    state_env(&mut c, dir);
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
            .success(),
        "git {args:?}"
    );
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-find-ids-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Test User"],
        &["config", "user.email", "t@example.com"],
    ] {
        git(&d, args);
    }
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

/// `add` a note on its own file; returns the new row's id (stdout's first token).
fn add(d: &Path, name: &str, text: &str) -> String {
    std::fs::write(d.join("src").join(name), "// x\n").unwrap();
    let (ok, out, err) = fael(d, &["add", "note", text, "--files", &format!("src/{name}")]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// Flip the id's last char to another valid Crockford char: a 26-char token
/// no row owns, so it is `Missing`, never `Many`.
fn phantom_of(id: &str) -> String {
    let mut f = id.to_string();
    let last = if f.ends_with('A') { 'B' } else { 'A' };
    f.pop();
    f.push(last);
    assert!(fael_core::looks_like_id(&f), "{f}");
    f
}

#[test]
fn real_id_shows_the_row() {
    let d = repo();
    let id = add(&d, "a.rs", "keeper row with a body");
    let (ok, out, err) = fael(&d, &["find", &id]);
    assert!(ok && out.contains("keeper row with a body"), "{err}{out}");
    // a unique id-shaped prefix is the same row, not a text search
    let (ok, out, err) = fael(&d, &["find", &id[..12]]);
    assert!(ok && out.contains("keeper row with a body"), "{err}{out}");
}

#[test]
fn fake_id_mentioned_elsewhere_rejects_and_names_the_mentioner() {
    let d = repo();
    let id = add(&d, "a.rs", "context row");
    let fake = phantom_of(&id);
    let citing = add(&d, "b.rs", &format!("see {fake} for context"));
    let (ok, out, err) = fael(&d, &["find", &fake]);
    assert!(!ok, "{out}");
    assert!(
        err.contains(&format!("rejected: no row with id \"{fake}\""))
            && err.contains("copy the id from fael find"),
        "{err}"
    );
    // the citing row is named by its short id — a mention is not ownership
    assert!(
        err.contains("mentioned (not owned) by:") && err.contains(&citing[..8]),
        "{err}"
    );
}

#[test]
fn fake_id_with_text_flag_searches_literally() {
    let d = repo();
    let id = add(&d, "a.rs", "context row");
    let fake = phantom_of(&id);
    add(&d, "b.rs", &format!("see {fake} for context"));
    let (ok, out, err) = fael(&d, &["find", "--text", &fake]);
    assert!(ok && out.contains("for context"), "{err}{out}");
}

#[test]
fn ambiguous_prefix_rejects() {
    let d = repo();
    // rapid adds share the 8-char timestamp head — stop at the first pair
    let mut ids = vec![];
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
    let (ok, out, err) = fael(&d, &["find", &prefix]);
    assert!(!ok, "{out}");
    assert!(
        err.contains("matches 2 rows") && err.contains("use more characters"),
        "{err}"
    );
}

#[test]
fn non_id_shaped_query_stays_text_search() {
    let d = repo();
    add(&d, "a.rs", "keeper row");
    // plain words never id-match
    let (ok, out, err) = fael(&d, &["find", "no-such-words-anywhere"]);
    assert!(
        ok && out.is_empty() && err.contains("no rows match"),
        "{err}{out}"
    );
    // too short to be id-shaped (7 chars) — text search, not a reject
    let (ok, out, err) = fael(&d, &["find", "01ABCDE"]);
    assert!(
        ok && out.is_empty() && err.contains("no rows match"),
        "{err}{out}"
    );
}

#[test]
fn printed_abbreviation_resolves_despite_a_close_row_collision() {
    let d = repo();
    // a close row's own id shares the open row's 8-char prefix — fael prints
    // `01AAAA00` for the open row, so finding by that printed id must not
    // reject as ambiguous (ref_state prefers rows; abbrev/resolve see rows only)
    std::fs::create_dir_all(d.join(".fael/log/t")).unwrap();
    std::fs::write(
        d.join(".fael/log/t/2026-09.jsonl"),
        "{\"v\":1,\"id\":\"01AAAA00000000000000000001\",\"ts\":\"2026-09-01T00:00:00.000Z\",\
         \"by\":\"t\",\"kind\":\"note\",\"text\":\"open keeper\",\"files\":[\"src/a.rs\"]}\n",
    )
    .unwrap();
    std::fs::write(
        d.join(".fael/log/t/2026-09.close.jsonl"),
        "{\"v\":1,\"id\":\"01AAAA00000000000000000002\",\"ts\":\"2026-09-01T00:00:00.010Z\",\
         \"by\":\"t\",\"kind\":\"close\",\"text\":\"fixed\",\
         \"reference\":\"01AAAA00000000000000000001\",\"files\":[]}\n",
    )
    .unwrap();
    let (ok, out, err) = fael(&d, &["find", "01AAAA00000000000000000001"]);
    assert!(ok && out.contains("[01AAAA00]"), "{err}{out}");
    let (ok, out, err) = fael(&d, &["find", "01AAAA00"]);
    assert!(ok && out.contains("open keeper"), "{err}{out}");
}

#[test]
fn fake_id_in_a_title_is_named_as_a_mention() {
    let d = repo();
    let id = add(&d, "a.rs", "context row");
    let fake = phantom_of(&id);
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, out, err) = fael(
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
    let citing = out.split_whitespace().next().unwrap();
    let (ok, out, err) = fael(&d, &["find", &fake]);
    assert!(!ok, "{out}");
    assert!(
        err.contains("mentioned (not owned) by:") && err.contains(&citing[..8]),
        "{err}"
    );
}

#[test]
fn id_only_on_another_clones_unmerged_branch_is_found() {
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

    // the union has no such row — only the escalation finds it, tagged
    let (ok, out, _) = fael(&d, &["find"]);
    assert!(ok && !out.contains("row C on feat"), "{out}");
    let (ok, out, err) = fael(&d, &["find", &cid]);
    assert!(ok && out.contains("row C on feat"), "{err}{out}");
    assert!(out.contains("@feat/y"), "{out}");
}
