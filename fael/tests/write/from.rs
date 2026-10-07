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
fn from_takes_only_user() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add", "decision", "x", "--files", "src/a.rs", "--from", "agent",
        ],
        "",
    );
    assert!(!ok && err.contains("the only value is `user`"), "{err}");
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
