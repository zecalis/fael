//! `add --supersedes <id> --replace old --with new`: re-file a row with one
//! passage changed, never a guess about which one.

use super::{fael, repo, row_json};

fn add(d: &std::path::Path, args: &[&str]) -> (bool, String, String) {
    let mut v = vec!["add"];
    v.extend_from_slice(args);
    fael(d, &v, "")
}

#[test]
fn replace_refiles_with_one_passage_changed() {
    let d = repo();
    let (ok, out, err) = add(
        &d,
        &[
            "decision",
            "use sqlite for the cache, it is simple",
            "--files",
            "doc:a",
            "--key",
            "db:store",
            "--title",
            "Pick storage",
        ],
    );
    assert!(ok, "{err}");
    let old = out.split_whitespace().next().unwrap().to_string();

    let (ok, _, err) = add(
        &d,
        &[
            "decision",
            "--supersedes",
            &old,
            "--replace",
            "sqlite",
            "--with",
            "postgres",
        ],
    );
    assert!(ok, "{err}");
    let new = row_json(&d, "use postgres");
    assert_eq!(new["text"], "use postgres for the cache, it is simple");
    assert_eq!(new["files"][0], "doc:a");
    assert_eq!(new["key"], "db:store");
    assert_eq!(new["title"], "Pick storage");
    // the old body is hidden, the new one is the open row
    let (_, list, _) = fael(&d, &["find", "--key", "db:store"], "");
    assert!(
        list.contains("Pick storage") && list.matches("- [").count() == 1,
        "{list}"
    );
}

#[test]
fn replace_rejects_what_it_cannot_place_exactly() {
    let d = repo();
    let (ok, out, err) = add(
        &d,
        &[
            "decision",
            "retry twice, then retry again",
            "--files",
            "doc:a",
        ],
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let rejected = |args: &[&str], want: &str| {
        let (ok, _, err) = add(&d, args);
        assert!(!ok && err.contains(want), "{args:?}: {err}");
    };
    rejected(
        &[
            "decision",
            "--supersedes",
            &id,
            "--replace",
            "never",
            "--with",
            "x",
        ],
        "is not in the body",
    );
    rejected(
        &[
            "decision",
            "--supersedes",
            &id,
            "--replace",
            "retry",
            "--with",
            "x",
        ],
        "occurs 2 times",
    );
    rejected(
        &["decision", "--supersedes", &id, "--replace", "twice"],
        "needs --with",
    );
    rejected(
        &[
            "decision",
            "--supersedes",
            &id,
            "--replace",
            "",
            "--with",
            "x",
        ],
        "not an empty string",
    );
    rejected(
        &["decision", "--replace", "twice", "--with", "x"],
        "add --supersedes",
    );
    rejected(
        &[
            "note",
            "--supersedes",
            &id,
            "--replace",
            "twice",
            "--with",
            "x",
        ],
        "is a decision",
    );
    rejected(
        &[
            "decision",
            "some text",
            "--supersedes",
            &id,
            "--replace",
            "twice",
            "--with",
            "x",
        ],
        "drop the",
    );
    // every reject left the original row open and alone
    let (_, list, _) = fael(&d, &["find", "--files", "doc:a"], "");
    assert_eq!(list.matches("- [").count(), 1, "{list}");
}
