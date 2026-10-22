//! `from: user` — whose call a row records. Only `user` is a value; lists
//! say `(from user)`, and a row that replaces the user's call without being
//! the user's warns.

use super::{fael, repo};

#[test]
fn from_user_is_stored_and_listed() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let add = [
        "add",
        "decision",
        "rounding is a switch",
        "--files",
        "src/a.rs",
    ];
    let (ok, _, err) = fael(&d, &[&add[..], &["--from", "User"]].concat(), "");
    assert!(ok, "{err}");
    let (ok, out, _) = fael(&d, &["find", "rounding"], "");
    assert!(
        ok && out.contains("rounding is a switch (from user) →"),
        "{out}"
    );
    let (ok, out, _) = fael(&d, &["find", "rounding", "--json"], "");
    assert!(ok && out.contains(r#""from":"user""#), "{out}");
}

#[test]
fn from_takes_user_or_report() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add", "decision", "x", "--files", "src/a.rs", "--from", "agent",
        ],
        "",
    );
    assert!(!ok && err.contains("the values are `user`"), "{err}");
}

#[test]
fn superseding_the_users_call_warns_unless_the_user_said_it() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "rounding is a switch",
            "--files",
            "src/a.rs",
            "--from",
            "user",
        ],
        "",
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let re = |extra: &[&str]| {
        let base = [
            "add",
            "decision",
            "rounding is always on",
            "--files",
            "src/a.rs",
        ];
        fael(&d, &[&base[..], &["--supersedes", &id], extra].concat(), "")
    };
    let (ok, _, err) = re(&["--dry-run"]);
    assert!(ok && err.contains("the user's call"), "{err}");
    let (ok, _, err) = re(&["--from", "user"]);
    assert!(ok && !err.contains("the user's call"), "{err}");
}

#[test]
fn a_report_row_is_labelled_and_bump_drops_it() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "audit says x is broken",
            "--files",
            "src/a.rs",
            "--from",
            "report",
        ],
        "",
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let (_, out, _) = fael(&d, &["find", "audit says"], "");
    assert!(out.contains("(from report, unchecked)"), "{out}");
    let (ok, _, err) = fael(&d, &["bump", &id], "");
    assert!(ok && err.contains("was from a report, unchecked"), "{err}");
    let (_, out, _) = fael(&d, &["find", "audit says"], "");
    assert!(!out.contains("from report"), "{out}");
    // a user row is untouched by bump
    let (_, out, _) = fael(
        &d,
        &[
            "add",
            "decision",
            "user call",
            "--files",
            "src/a.rs",
            "--from",
            "user",
        ],
        "",
    );
    let uid = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(&d, &["bump", &uid], "");
    assert!(ok && !err.contains("from a report"), "{err}");
    let (_, out, _) = fael(&d, &["find", "user call"], "");
    assert!(out.contains("(from user)"), "{out}");
}

#[test]
fn a_new_issue_lists_open_rows_on_its_files() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    for t in ["one", "two", "three"] {
        let (ok, _, err) = fael(
            &d,
            &[
                "add",
                "decision",
                &format!("choice {t}"),
                "--files",
                "src/a.rs",
                "--title",
                &format!("title {t}"),
            ],
            "",
        );
        assert!(ok, "{err}");
    }
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "something broke", "--files", "src/a.rs"],
        "",
    );
    assert!(ok && err.matches("on these files: ").count() == 2, "{err}");
}
