//! `fael chunk ask|reply|answer` through the real binary: two runs (`--no-worktree`, named by
//! `--run R`) agree on a shape, counter twice into the owner, and show it in `chunk show`.

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
fn ask_counter_agree_and_the_holder_sees_it() {
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
    // the asker's run goes on: it asks again, and it still holds a
    let (_, show, _) = fael(&d, &["chunk", "show", &b]);
    assert!(show.contains(&format!("ask {id} ← {a}")), "{show}");
    assert!(show.contains("  check: cargo test col"), "{show}");
    // only the holder answers an open ask
    let (ok, _, err) = fael(&d, &["chunk", "reply", &id, "agree", "--run", "RA"]);
    assert!(!ok && err.contains("answers it"), "{err}");
    let (ok, out, err) = fael(
        &d,
        &[
            "chunk",
            "reply",
            &id,
            "counter",
            "fn col() -> u16",
            "u8 overflows",
            "--run",
            "RB",
        ],
    );
    assert!(ok && out.contains("→ countered"), "{out}{err}");
    let (ok, out, err) = fael(&d, &["chunk", "reply", &id, "agree", "--run", "RA"]);
    assert!(ok && out.contains("→ agreed"), "{out}{err}");
    let (_, json, _) = fael(&d, &["chunk", "show", &a, "--json"]);
    assert!(json.contains("\"returns\": \"fn col() -> u16\""), "{json}");
    assert!(
        json.contains("\"state\": \"agreed\"") && json.contains("\"dir\": \"out\""),
        "{json}"
    );
}

#[test]
fn two_counters_wait_on_the_owner_and_chunk_answer_settles_it() {
    let (d, _, b) = lanes();
    let (_, out, _) = fael(
        &d,
        &["chunk", "ask", &b, "w", "--returns", "u8", "--run", "RA"],
    );
    let id = contract_id(&out);
    assert!(
        fael(
            &d,
            &[
                "chunk", "reply", &id, "counter", "u16", "why", "--run", "RB"
            ]
        )
        .0
    );
    let (ok, out, _) = fael(
        &d,
        &[
            "chunk", "reply", &id, "counter", "u32", "why not", "--run", "RA",
        ],
    );
    assert!(
        ok && out.contains("waits on the owner") && out.contains("chunk answer"),
        "{out}"
    );
    let (ok, out, err) = fael(&d, &["chunk", "answer", &id, "u64"]);
    assert!(ok && out.contains("→ agreed"), "{out}{err}");
    // a chunk uid still goes the owner's way
    let (ok, _, err) = fael(&d, &["chunk", "answer", &b, "x"]);
    assert!(
        !ok && err.contains("`answer` replies to a waiting chunk"),
        "{err}"
    );
}

#[test]
fn a_loop_a_missing_shape_and_a_withdraw() {
    let (d, a, b) = lanes();
    let (ok, _, err) = fael(&d, &["chunk", "ask", &b, "w", "--run", "RA"]);
    assert!(!ok && err.contains("--returns"), "{err}");
    let (_, out, _) = fael(
        &d,
        &["chunk", "ask", &b, "w", "--returns", "r", "--run", "RA"],
    );
    let id = contract_id(&out);
    let (ok, _, err) = fael(
        &d,
        &["chunk", "ask", &a, "w", "--returns", "r", "--run", "RB"],
    );
    assert!(!ok && err.contains("closes a loop"), "{err}");
    let (ok, out, err) = fael(&d, &["chunk", "ask", "withdraw", &id, "--run", "RA"]);
    assert!(ok && out.contains("→ withdrawn"), "{out}{err}");
    assert!(
        fael(
            &d,
            &["chunk", "ask", &a, "w", "--returns", "r", "--run", "RB"]
        )
        .0
    );
}
