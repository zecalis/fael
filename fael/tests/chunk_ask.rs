//! `fael chunk ask|reply|answer` through the real binary: two runs (`--no-worktree`, named by
//! `--run R`) agree or refuse, and `chunk show` lists the asks. The holder's answer is final.

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

/// Two running chunks, `(dir, a, b)`: a under run RA, b under run RB.
fn lanes() -> (PathBuf, String, String) {
    let d = std::env::temp_dir().join(format!("fael-ask-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let mut uids = Vec::new();
    for (t, run) in [("A", "RA"), ("B", "RB")] {
        let (ok, out, err) = fael(&d, &["chunk", "add", t, "--brief", "do it"]);
        assert!(ok, "{err}");
        let u = out.trim().to_string();
        let (ok, _, err) = fael(&d, &["chunk", "start", &u, "--run", run, "--no-worktree"]);
        assert!(ok, "{err}");
        uids.push(u);
    }
    (d, uids.remove(0), uids.remove(0))
}

fn contract_id(out: &str) -> String {
    out.split_whitespace().nth(1).unwrap().to_string()
}

#[test]
fn ask_agree_with_the_shape_the_holder_will_deliver() {
    let (d, a, b) = lanes();
    let (ok, out, err) = fael(
        &d,
        &[
            "chunk",
            "ask",
            &b,
            "add a column",
            "--returns",
            "fn col() -> u8",
            "--check",
            "cargo test col",
            "--run",
            "RA",
        ],
    );
    assert!(ok && out.contains("→ open"), "{out}{err}");
    let id = contract_id(&out);
    let (_, show, _) = fael(&d, &["chunk", "show", &b]);
    assert!(show.contains(&format!("ask {id} ← {a}")), "{show}");
    assert!(show.contains("  check: cargo test col"), "{show}");
    // only the holder answers
    let (ok, _, err) = fael(&d, &["chunk", "reply", &id, "agree", "--run", "RA"]);
    assert!(!ok && err.contains("answers it"), "{err}");
    let (ok, out, err) = fael(
        &d,
        &[
            "chunk",
            "reply",
            &id,
            "agree",
            "--returns",
            "fn col() -> u16",
            "--run",
            "RB",
        ],
    );
    assert!(ok && out.contains("→ agreed"), "{out}{err}");
    let (_, json, _) = fael(&d, &["chunk", "show", &a, "--json"]);
    assert!(json.contains("\"returns\": \"fn col() -> u16\""), "{json}");
    assert!(
        json.contains("\"state\": \"agreed\"") && json.contains("\"dir\": \"out\""),
        "{json}"
    );
    // final
    let (ok, _, err) = fael(
        &d,
        &["chunk", "reply", &id, "refuse", "late", "--run", "RB"],
    );
    assert!(
        !ok && err.contains("is agreed") && err.contains("final"),
        "{err}"
    );
}

#[test]
fn refuse_keeps_its_reason_and_a_loop_is_refused_at_once() {
    let (d, a, b) = lanes();
    let (_, out, _) = fael(
        &d,
        &["chunk", "ask", &b, "w", "--returns", "u8", "--run", "RA"],
    );
    let id = contract_id(&out);
    // b started later: its ask back to a closes a loop and is refused at once
    let (ok, out, err) = fael(
        &d,
        &["chunk", "ask", &a, "w", "--returns", "r", "--run", "RB"],
    );
    assert!(
        ok && out.contains("→ refused: ") && out.contains("older claim wins"),
        "{out}{err}"
    );
    let (ok, out, err) = fael(
        &d,
        &[
            "chunk",
            "reply",
            &id,
            "refuse",
            "schema is frozen",
            "--run",
            "RB",
        ],
    );
    assert!(ok && out.contains("→ refused"), "{out}{err}");
    let (_, show, _) = fael(&d, &["chunk", "show", &a]);
    assert!(show.contains("  refused: schema is frozen"), "{show}");
    // `answer` is no longer a contract's: a contract id is no chunk
    let (ok, _, err) = fael(&d, &["chunk", "answer", &id, "x"]);
    assert!(!ok && err.contains("no chunk"), "{err}");
}

#[test]
fn a_holder_that_is_gone_refuses_at_once_and_withdraw_works_while_open() {
    let (d, _, b) = lanes();
    let (ok, _, err) = fael(&d, &["chunk", "ask", &b, "w", "--run", "RA"]);
    assert!(!ok && err.contains("--returns"), "{err}");
    assert!(fael(&d, &["chunk", "done", &b, "out", "--run", "RB"]).0);
    let (ok, out, err) = fael(
        &d,
        &["chunk", "ask", &b, "w", "--returns", "r", "--run", "RA"],
    );
    assert!(
        ok && out.contains("→ refused: ") && out.contains("is review"),
        "{out}{err}"
    );
    // a is still running: it asks a live chunk and takes the ask back
    let (_, c, _) = fael(&d, &["chunk", "add", "C", "--brief", "c"]);
    let c = c.trim().to_string();
    assert!(fael(&d, &["chunk", "start", &c, "--run", "RC", "--no-worktree"]).0);
    let (_, out, _) = fael(
        &d,
        &["chunk", "ask", &c, "w", "--returns", "r", "--run", "RA"],
    );
    let id = contract_id(&out);
    let (ok, out, err) = fael(&d, &["chunk", "ask", "withdraw", &id, "--run", "RA"]);
    assert!(ok && out.contains("→ withdrawn"), "{out}{err}");
}
