//! `fael intake` (plan fael-intake i2): a reopened round's brief prints the round before, and
//! a ticket the source closed drops, skips or accepts its chunk by state — every row `intake`'s.

mod common;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(d: &Path, args: &[&str]) -> String {
    let o = Command::new(common::fael_bin())
        .args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", d.join("state"))
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
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-intake-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    let ok = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&d)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let d = d.canonicalize().unwrap();
    let cat = if cfg!(windows) {
        "type list.jsonl"
    } else {
        "cat list.jsonl"
    };
    let s = serde_json::json!({ "t": { "list": cat, "read": "show {ref}", "draft": "draft {ref}",
                                       "open": "https://staff.test/i/{ref}" } });
    std::fs::write(d.join(".fael/sources.json"), s.to_string()).unwrap();
    d
}

fn list(d: &Path, tickets: &[(&str, u32, Option<&str>)]) {
    let lines: Vec<String> = tickets
        .iter()
        .map(|(r, reopen, closed)| {
            serde_json::json!({"ref": r, "kind": "bug", "title": format!("{r} title"),
                               "reopen": reopen, "closed": closed})
            .to_string()
        })
        .collect();
    std::fs::write(d.join("list.jsonl"), lines.join("\n") + "\n").unwrap();
}

fn show(d: &Path, label: &str) -> Value {
    serde_json::from_str(&fael(d, &["chunk", "show", label, "--json"])).unwrap()
}

/// The uid of the chunk intake made for `label`.
fn uid(d: &Path, label: &str) -> String {
    show(d, label)["uid"].as_str().unwrap().to_string()
}

#[test]
fn a_reopened_round_prints_the_round_before() {
    let d = repo();
    list(&d, &[("V-0042", 0, None)]);
    fael(&d, &["intake"]);
    let r0 = uid(&d, "v-0042");
    fael(&d, &["chunk", "start", &r0, "--run", "R1", "--no-worktree"]);
    fael(
        &d,
        &[
            "chunk",
            "done",
            &r0,
            "guarded the null",
            "--pr",
            "7",
            "--run",
            "R1",
        ],
    );
    list(&d, &[("V-0042", 0, Some("closed")), ("V-0042", 1, None)]);
    let out = fael(&d, &["intake"]);
    assert!(out.contains("added V-0042 → "), "{out}");
    let b = show(&d, "v-0042-r1")["brief"].as_str().unwrap().to_string();
    assert!(
        b.contains("\nBefore (r0): guarded the null · PR 7\n"),
        "{b}"
    );
    // round 0 never started: no hand-off, no line
    list(&d, &[("V-0043", 1, None)]);
    fael(&d, &["intake"]);
    assert!(
        !show(&d, "v-0043-r1")["brief"]
            .as_str()
            .unwrap()
            .contains("Before")
    );
}

#[test]
fn a_ticket_the_source_closed_moves_its_chunk_by_state() {
    let d = repo();
    let all = ["V-1", "V-2", "V-3", "V-4"];
    list(&d, &all.map(|r| (r, 0, None)));
    fael(&d, &["intake"]);
    let u = all.map(|r| uid(&d, &r.to_lowercase()));
    fael(
        &d,
        &["chunk", "start", &u[1], "--run", "R2", "--no-worktree"],
    );
    fael(
        &d,
        &["chunk", "start", &u[2], "--run", "R3", "--no-worktree"],
    );
    fael(&d, &["chunk", "done", &u[2], "ready", "--run", "R3"]);
    // V-4 is gone from the list: absence is not a close
    list(
        &d,
        &[
            ("V-1", 0, Some("dup:V-0040")),
            ("V-2", 0, Some("closed")),
            ("V-3", 0, Some("closed")),
        ],
    );
    let out = fael(&d, &["intake"]);
    assert!(out.contains(&format!("dropped V-1 → {}", u[0])), "{out}");
    assert!(out.contains("skipped V-2 (running)"), "{out}");
    assert!(out.contains(&format!("accepted V-3 → {}", u[2])), "{out}");
    assert!(out.ends_with("2 change(s) from 3 ticket(s)\n"), "{out}");
    let state = |l: &str| show(&d, l)["state"].as_str().unwrap().to_string();
    assert_eq!(
        [state("v-1"), state("v-2"), state("v-3"), state("v-4")],
        ["dropped", "running", "done", "open"]
    );
    // the rerun finds done / dropped chunks and a running one: nothing new to write
    let out = fael(&d, &["intake"]);
    assert_eq!(out, "skipped V-2 (running)\n0 change(s) from 3 ticket(s)\n");
}
