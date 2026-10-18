//! `fael chunk show` with the forms agents type: `p/label`, `p:label`, `p label`, `--uid`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> (PathBuf, String) {
    let d = std::env::temp_dir().join(format!("fael-show-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    assert!(fael(&d, &["plan", "new", "pl", "PL"]).0);
    let (ok, uid, err) = fael(
        &d,
        &[
            "chunk", "add", "Post", "--plan", "pl", "--label", "b1", "--brief", "x",
        ],
    );
    assert!(ok, "{err}");
    (d, uid.trim().to_string())
}

#[test]
fn every_natural_form_shows_the_chunk() {
    let (d, uid) = repo();
    for args in [
        vec!["chunk", "show", "pl/b1"],
        vec!["chunk", "show", "pl:b1"],
        vec!["chunk", "show", "pl", "b1"],
        vec!["chunk", "show", "--uid", &uid],
        vec!["chunk", "show", "--plan", "pl", "b1"],
        vec!["chunk", "show", "b1"],
    ] {
        let (ok, out, err) = fael(&d, &args);
        assert!(ok && out.contains(&uid), "{args:?}: {out}{err}");
    }
}

#[test]
fn a_wrong_form_names_the_retype() {
    let (d, _) = repo();
    for args in [
        vec!["chunk", "show", "pl/zz"],
        vec!["chunk", "show", "pl", "zz"],
        vec!["chunk", "show", "nope:b1"],
    ] {
        let (ok, _, err) = fael(&d, &args);
        assert!(
            !ok && err.contains("retype `fael chunk show --plan"),
            "{args:?}: {err}"
        );
    }
}

fn fael_in(dir: &Path, state: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn git_init(d: &Path) {
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(d)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn show_elsewhere_names_the_repo_that_holds_the_uid() {
    let tmp = std::env::temp_dir().join(format!("fael-show-else-{}", fael_core::ulid()));
    let state = tmp.join("state");
    let (a, b) = (tmp.join("a"), tmp.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    git_init(&a);
    git_init(&b);
    assert!(fael_in(&a, &state, &["plan", "new", "pl", "PL"]).0);
    assert!(fael_in(&b, &state, &["plan", "new", "pl", "PL"]).0);
    let (ok, auid, err) = fael_in(
        &a,
        &state,
        &["chunk", "add", "A", "--plan", "pl", "--brief", "x"],
    );
    assert!(ok, "{err}");
    let auid = auid.trim().to_string();
    let (ok, buid, err) = fael_in(
        &b,
        &state,
        &["chunk", "add", "B", "--plan", "pl", "--brief", "y"],
    );
    assert!(ok, "{err}");
    let buid = buid.trim().to_string();

    // a full uid from the other repo: name its root and the cd retype, still reject
    let (ok, _, err) = fael_in(&a, &state, &["chunk", "show", &buid]);
    let broot = b.canonicalize().unwrap().to_string_lossy().into_owned();
    assert!(!ok, "show {buid} from A passed");
    assert!(err.contains(&buid), "{err}");
    assert!(err.contains(&broot), "{err}");
    assert!(
        err.contains(&format!("cd {broot} && fael chunk show {buid}")),
        "{err}"
    );

    // a long prefix of it does the same, never runs elsewhere
    let prefix = &buid[..20];
    assert_ne!(&auid[..20], prefix, "uids too close, lengthen the prefix");
    let (ok, _, err) = fael_in(&a, &state, &["chunk", "show", prefix]);
    assert!(!ok, "show {prefix} from A passed: {err}");
    assert!(err.contains(&broot), "{err}");

    // a label never resolves across repos
    let (ok, _, err) = fael_in(&a, &state, &["chunk", "show", "b1"]);
    assert!(!ok && !err.contains("lives in"), "{err}");

    // an unknown uid still rejects as before, with no elsewhere line
    let (ok, _, err) = fael_in(&a, &state, &["chunk", "show", "01M9999999999999999999999"]);
    assert!(!ok && err.contains("no chunk"), "{err}");
    assert!(!err.contains("lives in"), "{err}");

    // a dropped chunk says dropped
    assert!(fael_in(&a, &state, &["chunk", "drop", &auid, "bye"]).0);
    assert!(fael_in(&a, &state, &["plan", "park", "pl"]).0);
    let (ok, _, err) = fael_in(&a, &state, &["chunk", "show", &auid]);
    assert!(!ok, "parked show passed");
    assert!(err.contains("is not on the board"), "{err}");
    assert!(err.contains("dropped"), "{err}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn show_in_a_parked_plan_names_the_plan() {
    let (d, uid) = repo();
    let state = d.join("state");
    assert!(fael_in(&d, &state, &["plan", "park", "pl"]).0);
    let (ok, _, err) = fael_in(&d, &state, &["chunk", "show", &uid]);
    assert!(!ok);
    assert!(err.contains("is not on the board"), "{err}");
    assert!(err.contains("parked"), "{err}");
    assert!(err.contains("pl"), "{err}");
}
