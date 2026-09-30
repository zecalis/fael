//! The one-line public-origin warning: `store = local` with the destination
//! set to `origin`, and nothing else. The push always happens either way.

use super::*;

const WARN: &str = "publicly fetchable from origin";

/// `store = "local"` in `.fael/config.toml` — rows in the journal only.
fn local_store(d: &Path) {
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
}

#[test]
fn local_store_at_origin_warns_once_and_pushes_anyway() {
    let origin = bare("origin-warn");
    let d = repo("origin-warn-a", "Alice", "alice@example.com");
    local_store(&d);
    git(
        &d,
        &["config", "remote.origin.url", origin.to_str().unwrap()],
    );
    point(&d, &origin);
    let id = add(&d, "local store, public destination");

    let (ok, out, err) = sync(&d);
    assert!(ok, "{err} {out}");
    assert_eq!(
        err.matches(WARN).count(),
        1,
        "exactly one warning line: {err}"
    );
    assert_eq!(fael_refs(&origin).len(), 1, "the ref is pushed regardless");

    // `store = local` keeps rows out of the tree — that is the point of it
    assert!(!d.join(".fael/log").exists(), "local skips the tree log");
    assert!(ids(&d).contains(&id), "the row is in the journal");
}

#[test]
fn another_destination_or_another_store_stays_silent() {
    // `store = local`, but the destination is a private remote — no warning
    let origin = bare("origin-quiet");
    let private = bare("origin-private");
    let d = repo("origin-quiet-a", "Alice", "alice@example.com");
    local_store(&d);
    git(
        &d,
        &["config", "remote.origin.url", origin.to_str().unwrap()],
    );
    point(&d, &private);
    add(&d, "local store, private destination");

    let (ok, _, err) = sync(&d);
    assert!(ok, "{err}");
    assert!(!err.contains(WARN), "{err}");
    assert_eq!(
        fael_refs(&private).len(),
        1,
        "the private remote got the ref"
    );
    assert!(fael_refs(&origin).is_empty(), "origin never saw it");

    // the tracked store (rows in the tree, committed like any other file)
    // going to its own origin is expected to be public — nothing to say
    let e = repo("origin-tracked", "Bob", "bob@example.com");
    std::fs::create_dir_all(e.join(".fael")).unwrap();
    std::fs::write(e.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    git(
        &e,
        &["config", "remote.origin.url", origin.to_str().unwrap()],
    );
    point(&e, &origin);
    add(&e, "tracked store at origin");

    let (ok, _, err) = sync(&e);
    assert!(ok, "{err}");
    assert!(!err.contains(WARN), "{err}");
    assert_eq!(fael_refs(&origin).len(), 1, "the ref is pushed");
}

#[test]
fn a_remote_name_or_another_spelling_of_origin_still_warns() {
    let origin = bare("origin-spell");
    let url = origin.to_str().unwrap();
    // the name `origin`, the url with a trailing slash, and a url that is only
    // origin's pushurl (its fetch url is elsewhere)
    for (n, set) in ["origin", "slash", "pushurl"].into_iter().enumerate() {
        let d = repo(&format!("origin-spell-{n}"), "Alice", "alice@example.com");
        local_store(&d);
        match set {
            "origin" => {
                git(&d, &["config", "remote.origin.url", url]);
                git(&d, &["config", "fael.remote", "origin"]);
            }
            "slash" => {
                git(&d, &["config", "remote.origin.url", url]);
                git(&d, &["config", "fael.remote", &format!("{url}/")]);
            }
            _ => {
                git(
                    &d,
                    &["config", "remote.origin.url", "/nonexistent/fetch.git"],
                );
                git(&d, &["config", "remote.origin.pushurl", url]);
                git(&d, &["config", "fael.remote", url]);
            }
        }
        add(&d, "public destination, spelled differently");
        let (ok, _, err) = sync(&d);
        assert!(ok, "{set}: {err}");
        assert_eq!(err.matches(WARN).count(), 1, "{set}: {err}");
    }
}
