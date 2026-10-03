//! `fh` (PLAN-fael-file-hash chunk 1): a row remembers the blob id of each
//! real file it names, restamped on add/bump but never on claim.

use super::{fael, repo, row_json};
use std::path::Path;
use std::process::Command;

/// `git hash-object <f> | cut -c1-12`, the value `fh` must reproduce.
fn git_blob(d: &Path, f: &str) -> String {
    let o = Command::new("git")
        .args(["hash-object", f])
        .current_dir(d)
        .output()
        .unwrap();
    assert!(o.status.success(), "git hash-object {f}");
    String::from_utf8_lossy(&o.stdout).trim()[..12].to_string()
}

fn fh(d: &Path, needle: &str) -> serde_json::Value {
    row_json(d, needle)["fh"].clone()
}

#[test]
fn add_stamps_git_blob_id() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "fn a() {}\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "stamped", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    let map = fh(&d, "stamped");
    assert_eq!(map["src/a.rs"], git_blob(&d, "src/a.rs").as_str());
    // a 40-hex blob id, cut to 12
    assert_eq!(map["src/a.rs"].as_str().unwrap().len(), 12);
}

#[test]
fn crlf_stamps_like_lf() {
    let d = repo();
    std::fs::write(d.join("src/crlf.rs"), "a\r\nb\r\n").unwrap();
    std::fs::write(d.join("src/lf.rs"), "a\nb\n").unwrap();
    assert!(fael(&d, &["add", "note", "one", "--files", "src/crlf.rs"], "").0);
    assert!(fael(&d, &["add", "note", "two", "--files", "src/lf.rs"], "").0);
    assert_eq!(fh(&d, "one")["src/crlf.rs"], fh(&d, "two")["src/lf.rs"]);
}

#[test]
fn anchors_globs_dirs_and_missing_files_are_not_stamped() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    std::fs::create_dir_all(d.join("src/emptydir")).unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "mixed",
            "--files",
            "src/a.rs,plan:foo,doc:bar,src/*.rs,src/emptydir,src/gone.rs",
            "--force",
        ],
        "",
    );
    assert!(ok, "{err}");
    let map = fh(&d, "mixed");
    assert!(map.get("src/a.rs").is_some(), "{map}");
    for absent in [
        "plan:foo",
        "doc:bar",
        "src/*.rs",
        "src/emptydir",
        "src/gone.rs",
    ] {
        assert!(
            map.get(absent).is_none(),
            "{absent} must have no key: {map}"
        );
    }
}

#[test]
fn nothing_stampable_writes_no_fh_field() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "anchors only", "--files", "plan:foo,doc:bar"],
        "",
    );
    assert!(ok, "{err}");
    let row = row_json(&d, "anchors only");
    assert!(row.get("fh").is_none(), "{row}");
}

#[test]
fn more_than_eight_files_stamp_the_first_eight() {
    let d = repo();
    for i in 0..9 {
        std::fs::write(d.join(format!("src/f{i}.rs")), format!("// {i}\n")).unwrap();
    }
    let files: Vec<String> = (0..9).map(|i| format!("src/f{i}.rs")).collect();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "many", "--files", &files.join(",")],
        "",
    );
    assert!(ok, "{err}");
    let map = fh(&d, "many");
    assert_eq!(map.as_object().unwrap().len(), 8, "{map}");
    assert!(map.get("src/f8.rs").is_none(), "the 9th is dropped: {map}");
}

#[test]
fn oversize_file_is_not_stamped() {
    let d = repo();
    let big = vec![b'x'; 1024 * 1024 + 1];
    std::fs::write(d.join("src/big.rs"), &big).unwrap();
    std::fs::write(d.join("src/ok.rs"), "//\n").unwrap();
    assert!(
        fael(
            &d,
            &["add", "note", "sized", "--files", "src/big.rs,src/ok.rs"],
            ""
        )
        .0
    );
    let map = fh(&d, "sized");
    assert!(map.get("src/big.rs").is_none(), "{map}");
    assert!(map.get("src/ok.rs").is_some(), "{map}");
}

#[test]
fn claim_keeps_the_hash_and_bump_restamps() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let (ok, out, err) = fael(&d, &["add", "issue", "x", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let before = fh(&d, "x");

    // claim: the file is untouched, so even the same bytes are the point —
    // but the map must survive a claim verbatim, never be recomputed
    let (ok, out, err) = fael(&d, &["claim", &id], "");
    assert!(ok, "{err}");
    assert_eq!(fh(&d, "x"), before, "claim must carry the old map");
    // claim superseded `id`; bump the version it filed
    let claimed = out.split_whitespace().next().unwrap().to_string();

    // bump after an edit: the new version carries the new blob id
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let (ok, _, err) = fael(&d, &["bump", &claimed], "");
    assert!(ok, "{err}");
    assert_eq!(fh(&d, "x")["src/a.rs"], git_blob(&d, "src/a.rs").as_str());
    assert_ne!(fh(&d, "x"), before);
}

#[test]
fn replace_supersede_stamps_too() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "use sqlite for the cache",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    std::fs::write(d.join("src/a.rs"), "// v2\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "--supersedes",
            &id,
            "--replace",
            "sqlite",
            "--with",
            "postgres",
        ],
        "",
    );
    assert!(ok, "{err}");
    let map = fh(&d, "use postgres");
    assert_eq!(map["src/a.rs"], git_blob(&d, "src/a.rs").as_str());
}

#[test]
fn json_shows_fh_but_lists_do_not() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    assert!(fael(&d, &["add", "note", "shown", "--files", "src/a.rs"], "").0);
    let (_, list, _) = fael(&d, &["find", "--files", "src/a.rs"], "");
    assert!(!list.contains("fh"), "hash must not ride a list: {list}");
    let (_, full, _) = fael(&d, &["find", "--files", "src/a.rs", "--json"], "");
    assert!(full.contains("\"fh\""), "{full}");
}
