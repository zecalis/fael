//! An agent's cut of the board (`--plan`, `--open`) carries no `ledger`; the app's plain
//! `fael board --json` still does (issue 01M4TEHR).

use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(crate::common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn an_agents_cut_has_no_ledger_the_plain_board_does() {
    let tmp = std::env::temp_dir().join(format!("fael-lean-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let ok = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&d)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let d = d.canonicalize().unwrap();
    let f = |args: &[&str]| fael(&d, &state, args);
    f(&["add", "issue", "a breaks", "--files", "src/a.rs"]);
    f(&["plan", "new", "p", "P"]);
    f(&["chunk", "add", "x", "--plan", "p", "--brief", "b"]);

    let board = |extra: &[&str]| -> Value {
        let mut args = vec!["board", "--json"];
        args.extend_from_slice(extra);
        serde_json::from_str(&f(&args)).unwrap()
    };
    for extra in [
        &["--plan", "p"][..],
        &["--open"],
        &["--open", "--plan", "p"],
    ] {
        let v = board(extra);
        assert_eq!(v["projects"].as_array().unwrap().len(), 1, "{extra:?}");
        assert!(v["projects"][0].get("ledger").is_none(), "{extra:?}");
    }
    let v = board(&[]);
    assert!(v["projects"][0]["ledger"]["issues"].is_array());
}

/// `--project <name>` rereads one registered repo; a name nobody owns fails, never an empty
/// board that reads as live (issue 01M7TAZ1).
#[test]
fn project_cut_reads_one_repo_and_an_unknown_name_fails() {
    let tmp = std::env::temp_dir().join(format!("fael-project-{}", fael_core::ulid()));
    let state = tmp.join("state");
    let repo = |name: &str| {
        let d = tmp.join(name);
        std::fs::create_dir_all(&d).unwrap();
        let ok = Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let d = d.canonicalize().unwrap();
        fael(&d, &state, &["chunk", "add", "x", "--brief", "b"]);
        fael(&d, &state, &["board", "--json"]); // registers it
        d
    };
    let (a, _b) = (repo("alpha"), repo("beta"));
    let all: Value = serde_json::from_str(&fael(&a, &state, &["board", "--json"])).unwrap();
    assert_eq!(all["projects"].as_array().unwrap().len(), 2);
    let one: Value =
        serde_json::from_str(&fael(&a, &state, &["board", "--json", "--project", "beta"])).unwrap();
    let ps = one["projects"].as_array().unwrap();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0]["name"], "beta");
    let o = Command::new(crate::common::fael_bin())
        .args(["board", "--json", "--project", "nope"])
        .current_dir(&a)
        .env("FAEL_STATE_DIR", &state)
        .env_remove("FAEL_DIR")
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no registered project named nope"));
}
