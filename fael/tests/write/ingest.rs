//! `fael vault ingest` — human notes from `<vault>/inbox` become `from: user` rows.
//!
//! Happy path, the three failing cases (empty body, unknown kind, frontmatter
//! that does not parse), the content-hash dedupe key, and the shape gate
//! leaving a bad row in place.

use super::{fael, repo};
use serde_json::json;
use std::path::{Path, PathBuf};

fn inbox(d: &Path) -> PathBuf {
    let p = d.join(".git/fael/vault/inbox");
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn drop_note(d: &Path, name: &str, front: &str, body: &str) {
    std::fs::write(inbox(d).join(name), format!("---\n{front}---\n{body}")).unwrap();
}

/// Projected `.md` files under `<vault>/<repo>/note` — the write path's
/// `refresh` projects each ingested row in the same call.
fn projection_notes(d: &Path) -> Vec<PathBuf> {
    let vault = d.join(".git/fael/vault");
    let repo = std::fs::read_dir(&vault)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir() && p.file_name().is_some_and(|n| n != "inbox"))
        .unwrap();
    std::fs::read_dir(repo.join("note"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect()
}

fn count(d: &Path, needle: &str) -> usize {
    let (_, out, _) = fael(d, &["find", "--json", "--all"], "");
    out.lines().filter(|l| l.contains(needle)).count()
}

#[test]
fn happy_path_files_row_from_user_and_clears_inbox() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    drop_note(
        &d,
        "idea.md",
        "kind: note\ntitle: cache the kickoff\nfiles: src/a.rs\n",
        "cache the kickoff rows for five minutes\nmore detail here\n",
    );
    let (ok, out, err) = fael(&d, &["vault", "ingest"], "");
    assert!(ok, "{err}");
    assert!(out.contains("← inbox/idea.md"), "{out}");
    assert!(!inbox(&d).join("idea.md").exists());
    let id = out.split_whitespace().next().unwrap().to_string();
    let (_, found, _) = fael(&d, &["find", &id], "");
    assert!(found.contains("(from user)"), "{found}");
    let row = super::row_json(&d, "cache the kickoff");
    assert_eq!(row["kind"], json!("note"));
    assert_eq!(row["title"], json!("cache the kickoff"));
    assert_eq!(row["files"], json!(["src/a.rs"]));
    assert_eq!(
        row["text"],
        json!("cache the kickoff rows for five minutes\nmore detail here")
    );
    // the write path projects the new row as its inbox file disappears
    let proj = projection_notes(&d);
    assert_eq!(proj.len(), 1, "{proj:?}");
    let body = std::fs::read_to_string(&proj[0]).unwrap();
    assert!(
        body.contains("cache the kickoff rows for five minutes"),
        "{body}"
    );
    assert!(body.contains("from: \"user\""), "{body}");
}

#[test]
fn bare_frontmatter_falls_back_to_note_headline_anchor_and_hash_key() {
    let d = repo();
    drop_note(
        &d,
        "plain.md",
        "",
        "remember to water the plants tomorrow morning\nsecond line\n",
    );
    let (ok, _, err) = fael(&d, &["vault", "ingest"], "");
    assert!(ok, "{err}");
    let row = super::row_json(&d, "water the plants");
    assert_eq!(row["kind"], json!("note"));
    assert_eq!(
        row["title"],
        json!("remember to water the plants tomorrow morning")
    );
    assert_eq!(row["files"], json!(["doc:inbox"]));
    assert!(
        row["key"].as_str().is_some_and(|k| k.starts_with("vault:")),
        "dedupe key carries the content hash: {row}"
    );
}

#[test]
fn frontmatter_key_is_kept() {
    let d = repo();
    drop_note(&d, "k.md", "key: garden:water\n", "water the plants\n");
    let (ok, _, err) = fael(&d, &["vault", "ingest"], "");
    assert!(ok, "{err}");
    let row = super::row_json(&d, "water the plants");
    assert_eq!(row["key"], json!("garden:water"));
}

#[test]
fn unknown_kind_stays_with_reason() {
    let d = repo();
    drop_note(&d, "x.md", "kind: poem\n", "roses are red\n");
    let (ok, _, err) = fael(&d, &["vault", "ingest"], "");
    assert!(!ok, "unknown kind must exit non-zero");
    assert!(
        err.contains("inbox/x.md") && err.contains("unknown kind"),
        "{err}"
    );
    assert!(inbox(&d).join("x.md").exists());
    assert_eq!(count(&d, "roses are red"), 0);
}

#[test]
fn empty_body_stays_with_reason() {
    let d = repo();
    drop_note(&d, "empty.md", "kind: note\ntitle: nothing here\n", "   \n");
    let (ok, _, err) = fael(&d, &["vault", "ingest"], "");
    assert!(!ok, "empty body must exit non-zero");
    assert!(
        err.contains("inbox/empty.md") && err.contains("empty body"),
        "{err}"
    );
    assert!(inbox(&d).join("empty.md").exists());
}

#[test]
fn unparsable_frontmatter_stays_with_reason() {
    let d = repo();
    // never closes
    std::fs::write(
        inbox(&d).join("open.md"),
        "---\nkind: note\nbody without a closing fence\n",
    )
    .unwrap();
    // a line with no `key: value` shape
    drop_note(&d, "shape.md", "kind note\n", "body\n");
    // no frontmatter at all
    std::fs::write(inbox(&d).join("bare.md"), "just a body\n").unwrap();
    let (ok, _, err) = fael(&d, &["vault", "ingest"], "");
    assert!(!ok, "bad frontmatter must exit non-zero");
    for n in ["open.md", "shape.md", "bare.md"] {
        assert!(err.contains(n), "{err}");
        assert!(inbox(&d).join(n).exists());
    }
}

#[test]
fn shape_gate_reject_stays_with_reason() {
    let d = repo();
    drop_note(&d, "topics.md", "kind: issue\n", "alpha · beta · gamma\n");
    let (ok, _, err) = fael(&d, &["vault", "ingest"], "");
    assert!(!ok, "a topic list must exit non-zero");
    assert!(
        err.contains("inbox/topics.md") && err.contains("one topic per row"),
        "{err}"
    );
    assert!(inbox(&d).join("topics.md").exists());
    assert_eq!(count(&d, "alpha"), 0);
}

#[test]
fn second_ingest_of_same_bytes_files_nothing_twice() {
    let d = repo();
    let front = "kind: note\ntitle: same bytes\n";
    let body = "same bytes land once\n";
    drop_note(&d, "dup.md", front, body);
    let (ok, out, err) = fael(&d, &["vault", "ingest"], "");
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    assert_eq!(count(&d, "same bytes land once"), 1);
    drop_note(&d, "dup.md", front, body);
    let (ok, out, err) = fael(&d, &["vault", "ingest"], "");
    assert!(ok, "{err}");
    assert_eq!(out.split_whitespace().next().unwrap(), id);
    assert!(err.contains("already open"), "{err}");
    assert_eq!(count(&d, "same bytes land once"), 1);
    assert!(!inbox(&d).join("dup.md").exists());
}

#[test]
fn decision_kind_and_long_headline_truncated_to_15_words() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    drop_note(
        &d,
        "decide.md",
        "kind: Decision\nfiles: src/a.rs\n",
        "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen\nsecond line\n",
    );
    let (ok, _, err) = fael(&d, &["vault", "ingest"], "");
    assert!(ok, "{err}");
    let row = super::row_json(&d, "sixteen seventeen");
    assert_eq!(row["kind"], json!("decision"));
    assert_eq!(
        row["title"],
        json!(
            "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen"
        )
    );
}

#[test]
fn no_vault_yet_says_rebuild_first() {
    let d = repo();
    assert!(!d.join(".git/fael/vault").exists());
    let (ok, _, err) = fael(&d, &["vault", "ingest"], "");
    assert!(!ok, "no vault must exit non-zero");
    assert!(err.contains("run `fael vault rebuild` first"), "{err}");
    // after rebuild the empty inbox ingests clean
    let (ok, _, err) = fael(&d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    let (ok, out, err) = fael(&d, &["vault", "ingest"], "");
    assert!(ok, "{err}");
    assert!(out.is_empty(), "{out}");
}

#[test]
fn one_bad_file_leaves_the_good_one_filed() {
    let d = repo();
    drop_note(&d, "good.md", "", "a good note lands\n");
    drop_note(&d, "bad.md", "kind: poem\n", "a bad note stays\n");
    let (ok, out, err) = fael(&d, &["vault", "ingest"], "");
    assert!(!ok, "any failure exits non-zero: {err}");
    assert!(out.contains("← inbox/good.md"), "{out}");
    assert!(!inbox(&d).join("good.md").exists());
    assert!(inbox(&d).join("bad.md").exists());
    assert_eq!(count(&d, "a good note lands"), 1);
    assert_eq!(count(&d, "a bad note stays"), 0);
}
