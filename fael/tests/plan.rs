//! `fael plan new` / `export` / `apply` / `park` through the real binary: fael.db lands in
//! the git common dir, a read without a db is a reject, and the md-era verbs are gone.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-plan-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    d
}

fn write(p: PathBuf, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn outside_a_repo_says_so_and_leaves_no_db() {
    let d = std::env::temp_dir().join(format!("fael-norepo-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    for args in [
        &["plan", "export", "p"][..],
        &["chunk", "show", "b1"],
        &["plan", "new", "p", "P"],
    ] {
        let (ok, _, err) = fael(&d, args);
        assert!(
            !ok && err.contains("is in no git repo") && !err.contains("plan new"),
            "{args:?}: {err}"
        );
    }
    assert!(!d.join(".fael").exists(), "no stray fael.db");
}

#[test]
fn park_unpark_done_move_a_plan_and_unknown_names_are_refused() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["plan", "export"]);
    assert!(!ok && err.contains("fael plan new"), "{err}");
    assert!(fael(&d, &["plan", "new", "p", "P"]).0);
    assert!(d.join(".git/fael/fael.db").exists());
    let (ok, uid, err) = fael(&d, &["chunk", "add", "p1", "--plan", "p", "--brief", "b"]);
    assert!(ok, "{err}");
    let uid = uid.split_whitespace().next().unwrap();
    let (ok, out, err) = fael(&d, &["plan", "park", "p"]);
    assert!(ok && out == "plan p active → parked\n", "{err}{out}");
    let board = || fael(&d, &["board"]).1;
    assert!(!board().contains(uid), "{}", board());
    assert!(fael(&d, &["plan", "export", "p"]).1.contains("· parked"));
    let (ok, _, err) = fael(&d, &["plan", "park", "p"]);
    assert!(!ok && err.contains("p is parked"), "{err}");
    assert!(fael(&d, &["plan", "unpark", "p"]).0);
    assert!(board().contains(uid), "{}", board());
    // a running chunk keeps its plan on the board
    assert!(fael(&d, &["chunk", "start", uid]).0);
    let (ok, _, err) = fael(&d, &["plan", "done", "p"]);
    assert!(!ok && err.contains("1 running chunk(s)"), "{err}");
    assert!(fael(&d, &["chunk", "done", uid, "shipped", "--pr", "1"]).0);
    let (ok, out, err) = fael(&d, &["plan", "done", "p"]);
    assert!(ok && out == "plan p active → done\n", "{err}{out}");
    let (ok, _, err) = fael(&d, &["plan", "export", "nope"]);
    assert!(!ok && err.contains("no plan 'nope'"), "{err}");
    for gone in ["import", "next", "migrate"] {
        let (ok, _, err) = fael(&d, &["plan", gone]);
        assert!(!ok && err.contains("fael plan new"), "{gone}: {err}");
    }
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn plan_new_keeps_its_sections_in_the_db_and_a_bill_edits_the_plan() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["plan", "new", "n", "New work"]);
    assert!(ok && out.contains("plan n created"), "{err}{out}");
    let (ok, _, err) = fael(&d, &["plan", "new", "n", "again"]);
    assert!(!ok && err.contains("plan n already exists"), "{err}");
    let (ok, _, err) = fael(&d, &["plan", "new", "Bad Name", "x"]);
    assert!(!ok && err.contains("lowercase letters"), "{err}");
    let (ok, json, err) = fael(&d, &["plan", "export", "n", "--json"]);
    assert!(ok, "{err}");
    let mut bill: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        (bill["v"].as_i64(), bill["plan"].as_str()),
        (Some(1), Some("n"))
    );
    assert_eq!(bill["sections"][0]["title"], "1. Goal (why)", "{json}");
    // the planner fills a section and adds a chunk in one bill; --dry-run writes nothing
    bill["sections"] = serde_json::json!([{"key": "1", "body": "ship n"}]);
    bill["chunks"] = serde_json::json!([{"title": "n1", "brief": "do n1"}]);
    let file = d.join("bill.json");
    write(file.clone(), &bill.to_string());
    let path = file.to_str().unwrap();
    let (ok, out, err) = fael(&d, &["plan", "apply", path, "--dry-run"]);
    assert!(
        ok && out.contains("dry run: nothing written") && out.contains("+ "),
        "{err}{out}"
    );
    let (_, now, _) = fael(&d, &["plan", "export", "n", "--json"]);
    assert_eq!(now, json, "dry run changed the plan");
    let (ok, out, err) = fael(&d, &["plan", "apply", path]);
    assert!(ok && out.contains("applied (rev "), "{err}{out}");
    let (ok, out, err) = fael(&d, &["plan", "export", "n", "--section", "1"]);
    assert!(
        ok && out.starts_with("## 1. Goal (why)\n> section 1 · updated ")
            && out.ends_with("\nship n\n"),
        "{err}{out}"
    );
    // the same bill again is stale: the plan moved on
    let (ok, _, err) = fael(&d, &["plan", "apply", path]);
    assert!(!ok && err.contains("stale rev"), "{err}");
    let (_, board, _) = fael(&d, &["board", "--json"]);
    let board: serde_json::Value = serde_json::from_str(&board).unwrap();
    let uid = board["projects"][0]["chunks"][0]["uid"].as_str().unwrap();
    let (ok, out, err) = fael(&d, &["chunk", "start", uid]);
    assert!(
        ok && out.contains("<!-- plan n -->\n\n## 1. Goal (why)\n> section 1 · updated ")
            && out.contains("\nship n\n"),
        "{err}{out}"
    );
    let _ = std::fs::remove_dir_all(d);
}

/// What agents type for a plan or chunk: `plan show` reads it, and the rest is
/// a reject naming the call that works — `plan list`, `chunk show --plan`, a
/// ledger row's id, a flag and its value in one argument.
#[test]
fn the_guessed_plan_and_chunk_calls_answer_or_name_the_real_one() {
    let d = repo();
    assert!(fael(&d, &["plan", "new", "p", "P"]).0);
    let (ok, uid, err) = fael(&d, &["chunk", "add", "p1", "--plan", "p"]);
    assert!(ok, "{err}");
    let uid = uid.trim();
    let (ok, out, err) = fael(&d, &["plan", "show", "p"]);
    assert!(ok && out == fael(&d, &["plan", "export", "p"]).1, "{err}");
    let (ok, _, err) = fael(&d, &["plan", "list"]);
    assert!(!ok && err.contains("`fael board` lists the plans"), "{err}");
    // --plan names the plan a label or uid is in, and checks it
    assert!(fael(&d, &["chunk", "edit", uid, "--label", "l1"]).0);
    let (ok, out, err) = fael(&d, &["chunk", "show", "l1", "--plan", "p"]);
    assert!(ok && out.contains(uid), "{err}");
    assert!(fael(&d, &["chunk", "note", uid, "hi", "--plan", "p"]).0);
    assert!(fael(&d, &["plan", "new", "q", "Q"]).0);
    let (ok, _, err) = fael(&d, &["chunk", "show", uid, "--plan", "q"]);
    assert!(!ok && err.contains("is in plan p, not q"), "{err}");
    let (ok, _, err) = fael(&d, &["chunk", "add", "p2", "--plan p"]);
    assert!(!ok && err.contains("is one argument"), "{err}");
    write(d.join("a.rs"), "x\n");
    let (ok, id, err) = fael(&d, &["add", "note", "n", "--files", "a.rs"]);
    assert!(ok, "{err}");
    let id = id.split_whitespace().next().unwrap();
    let (ok, _, err) = fael(&d, &["chunk", "show", id]);
    assert!(
        !ok && err.contains("is a ledger note, not a chunk"),
        "{err}"
    );
}
