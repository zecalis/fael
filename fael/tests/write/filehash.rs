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
fn large_file_is_stamped_like_git_and_crlf_like_lf() {
    let d = repo();
    let line = "fn f() { let x = 1; }\n";
    let lf = line.repeat(100_000); // ~2 MiB, past the old 1 MiB cap
    std::fs::write(d.join("src/big.rs"), &lf).unwrap();
    std::fs::write(d.join("src/bigcrlf.rs"), lf.replace('\n', "\r\n")).unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "large",
            "--files",
            "src/big.rs,src/bigcrlf.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    let map = fh(&d, "large");
    assert_eq!(map["src/big.rs"], git_blob(&d, "src/big.rs").as_str());
    assert_eq!(map["src/bigcrlf.rs"], map["src/big.rs"]);
}

#[test]
fn oversize_file_is_not_stamped() {
    let d = repo();
    // sparse: one byte past the 16 MiB cap, without writing 16 MiB
    let big = std::fs::File::create(d.join("src/big.rs")).unwrap();
    big.set_len(16 * 1024 * 1024 + 1).unwrap();
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
fn claim_and_routing_bump_keep_the_hash_and_bare_bump_restamps() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let (ok, out, err) = fael(&d, &["add", "issue", "x", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let before = fh(&d, "x");

    // the file moves on, then the row is claimed: a claim is not a check, so
    // the map must be the old one, not a recompute of the edited bytes
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let (ok, out, err) = fael(&d, &["claim", &id], "");
    assert!(ok, "{err}");
    assert_eq!(fh(&d, "x"), before, "claim must carry the old map");
    let claimed = out.split_whitespace().next().unwrap().to_string();

    // re-routing is not a check either
    let (ok, out, err) = fael(&d, &["bump", &claimed, "--urgent"], "");
    assert!(ok, "{err}");
    assert_eq!(fh(&d, "x"), before, "a routing bump must carry the old map");
    let routed = out.split_whitespace().next().unwrap().to_string();

    // a bare bump is the "still true" check: it restamps from disk
    let (ok, _, err) = fael(&d, &["bump", &routed], "");
    assert!(ok, "{err}");
    assert_eq!(fh(&d, "x")["src/a.rs"], git_blob(&d, "src/a.rs").as_str());
    assert_ne!(fh(&d, "x"), before);
}

#[test]
fn bracketed_real_path_is_stamped() {
    let d = repo();
    std::fs::create_dir_all(d.join("src/[id]")).unwrap();
    std::fs::write(d.join("src/[id]/page.rs"), "//\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "dynamic", "--files", "src/[id]/page.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(fh(&d, "dynamic").get("src/[id]/page.rs").is_some());
}

#[test]
fn binary_hashes_like_git() {
    let d = repo();
    std::fs::write(d.join("src/b.bin"), b"\0a\r\nb\r\n").unwrap();
    assert!(fael(&d, &["add", "note", "bin", "--files", "src/b.bin"], "").0);
    assert_eq!(
        fh(&d, "bin")["src/b.bin"],
        git_blob(&d, "src/b.bin").as_str()
    );
}

#[test]
fn fh_does_not_count_toward_the_row_cap() {
    let d = repo();
    let files: Vec<String> = (0..8).map(|i| format!("src/long_name_{i:02}.rs")).collect();
    for f in &files {
        std::fs::write(d.join(f), "//\n").unwrap();
    }
    // 10_000 bytes of text leaves under 200 for the line's other fields — the
    // eight stamped paths alone would overflow the 10 KiB cap
    let text = format!("a{}", "b".repeat(9_900));
    let (ok, _, err) = fael(
        &d,
        &["add", "note", &text, "--files", &files.join(","), "--force"],
        "",
    );
    assert!(ok, "{err}");
    assert_eq!(fh(&d, "ab").as_object().unwrap().len(), 8);
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

/// The one info line `add`/bare `bump` print when a real file gets no key.
const NOTE: &str = "fael: not stamped (no file-hash verdict at push): ";

/// A sparse file one byte past the 16 MiB stamp cap, without writing 16 MiB.
fn sparse_over_cap(d: &Path, rel: &str) {
    let big = std::fs::File::create(d.join(rel)).unwrap();
    big.set_len(16 * 1024 * 1024 + 1).unwrap();
}

#[test]
fn oversize_file_is_named_in_the_receipt_and_the_row_is_still_recorded() {
    let d = repo();
    sparse_over_cap(&d, "src/big.bin");
    std::fs::write(d.join("src/ok.rs"), "//\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &["add", "note", "sized", "--files", "src/big.bin,src/ok.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(
        err.contains(&format!("{NOTE}src/big.bin (over 16 MiB)")),
        "{err}"
    );
    assert!(
        !err.contains("src/ok.rs"),
        "a stamped file is not named: {err}"
    );
    // the row is filed, and the receipt still starts with its id
    assert!(row_json(&d, "sized")["fh"].get("src/ok.rs").is_some());
    assert!(out.split_whitespace().next().unwrap().len() >= 26, "{out}");
}

#[test]
fn ninth_real_file_is_named_as_past_the_cap() {
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
    assert!(
        err.contains(&format!("{NOTE}src/f8.rs (past the 8-file cap)")),
        "{err}"
    );
    assert!(
        !err.contains("src/f7.rs"),
        "only the 9th is left out: {err}"
    );
    assert_eq!(fh(&d, "many").as_object().unwrap().len(), 8);
}

#[cfg(unix)]
#[test]
fn unreadable_file_is_named() {
    use std::os::unix::fs::PermissionsExt;
    let d = repo();
    let p = d.join("src/locked.rs");
    std::fs::write(&p, "//\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&p).is_ok() {
        return; // running as root: the mode bits do not bite
    }
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "locked", "--files", "src/locked.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(
        err.contains(&format!("{NOTE}src/locked.rs (unreadable)")),
        "{err}"
    );
    assert!(row_json(&d, "locked").get("fh").is_none());
}

#[test]
fn a_normal_row_and_the_legitimate_gaps_print_no_extra_line() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    std::fs::create_dir_all(d.join("src/emptydir")).unwrap();
    let (ok, out, err) = fael(&d, &["add", "note", "plain", "--files", "src/a.rs"], "");
    assert!(ok && !err.contains("not stamped"), "{err}");
    assert_eq!(out.lines().count(), 1, "{out}");
    // anchors, globs, directories and missing files are not the author's gap
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "gaps",
            "--files",
            "plan:foo,src/*.rs,src/emptydir,src/gone.rs",
            "--force",
        ],
        "",
    );
    assert!(ok && !err.contains("not stamped"), "{err}");
}

#[test]
fn bare_bump_says_what_it_left_out_but_claim_and_routing_bump_do_not() {
    let d = repo();
    sparse_over_cap(&d, "src/big.bin");
    let (ok, out, err) = fael(&d, &["add", "issue", "x", "--files", "src/big.bin"], "");
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(&d, &["claim", &id], "");
    assert!(ok && !err.contains("not stamped"), "{err}");
    let claimed = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(&d, &["bump", &claimed, "--urgent"], "");
    assert!(ok && !err.contains("not stamped"), "{err}");
    let routed = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(&d, &["bump", &routed], "");
    assert!(ok, "{err}");
    assert!(
        err.contains(&format!("{NOTE}src/big.bin (over 16 MiB)")),
        "{err}"
    );
}
