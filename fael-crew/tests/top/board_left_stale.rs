//! `fael board --json` chunk `left` (ids on the latest hand-off's `left:` line) and `stale`
//! (closed rows the title+brief cite, live chunks only) — both `[]` when empty.

use serde_json::{Value, json};
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

fn chunk(v: &Value, u: &str) -> Value {
    v["projects"][0]["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["uid"] == u)
        .unwrap()
        .clone()
}

#[test]
fn left_and_stale_ride_the_board() {
    let tmp = std::env::temp_dir().join(format!("fael-board-ls-{}", fael_core::ulid()));
    let d = tmp.join("repo");
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(d.join("a.rs"), "").unwrap();
    let state = tmp.join("state");
    let f = |args: &[&str]| fael(&d, &state, args);
    let first = |s: String| s.split_whitespace().next().unwrap().to_string();

    let shut = first(f(&["add", "issue", "old bug", "--files", "a.rs"]));
    f(&["close", &shut, "old bug → fixed; guard `src/a_test.rs`"]);
    let open = first(f(&["add", "issue", "still broken", "--files", "a.rs"]));

    let live = first(f(&[
        "chunk",
        "add",
        "live",
        "--brief",
        &format!("see {shut}"),
    ]));
    let clean = first(f(&["chunk", "add", "clean", "--brief", "nothing cited"]));
    let none = first(f(&["chunk", "add", "none", "--brief", "b"]));
    let both = first(f(&[
        "chunk",
        "add",
        "both",
        "--brief",
        &format!("see {shut}"),
    ]));
    f(&["chunk", "start", &none, "--no-worktree"]);
    f(&[
        "chunk", "done", &none, "shipped", "--left", "none", "--pr", "1",
    ]);
    f(&["chunk", "start", &both, "--no-worktree", "--force"]);
    f(&[
        "chunk",
        "done",
        &both,
        "shipped",
        "--left",
        &format!("{open},{live}"),
        "--pr",
        "2",
    ]);

    let v: Value = serde_json::from_str(&f(&["board", "--json"])).unwrap();
    assert_eq!(chunk(&v, &both)["left"], json!([open, live]));
    assert_eq!(
        chunk(&v, &both)["left_open"],
        json!([open, live]),
        "both ids are still open"
    );
    f(&[
        "close",
        &open,
        "still broken → fixed; guard `src/a_test.rs`",
    ]);
    let v: Value = serde_json::from_str(&f(&["board", "--json"])).unwrap();
    assert_eq!(
        chunk(&v, &both)["left"],
        json!([open, live]),
        "left keeps history"
    );
    assert_eq!(
        chunk(&v, &both)["left_open"],
        json!([live]),
        "a closed row leaves left_open"
    );
    assert_eq!(
        chunk(&v, &both)["stale"],
        json!([]),
        "a done chunk is not checked"
    );
    assert_eq!(chunk(&v, &none)["left"], json!([]));
    assert_eq!(chunk(&v, &clean)["left"], json!([]), "no hand-off");
    assert_eq!(chunk(&v, &clean)["stale"], json!([]));
    let stale = chunk(&v, &live)["stale"].clone();
    assert_eq!(stale.as_array().unwrap().len(), 1, "{stale}");
    assert!(stale[0].as_str().unwrap().contains(&shut), "{stale}");
}
