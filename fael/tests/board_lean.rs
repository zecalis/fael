//! An agent's cut of the board (`--plan`, `--open`) carries no `ledger`; the app's plain
//! `fael board --json` still does (issue 01M4TEHR).

use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
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
