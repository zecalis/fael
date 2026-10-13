//! `fael intake` (plan fael-intake i1) through the real binary with a fixture adapter whose
//! `list` prints a JSONL file: one chunk per ticket round in any plan, adoption of a chunk an
//! older sync made, the brief per kind, and every failing list writes nothing.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(d: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap()
}

fn fael(d: &Path, args: &[&str]) -> String {
    let o = run(d, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A git repo whose source `t` lists `list.jsonl` (`type` under cmd /C on Windows).
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
    sources(
        &d,
        if cfg!(windows) {
            "type list.jsonl"
        } else {
            "cat list.jsonl"
        },
    );
    d
}

fn sources(d: &Path, list: &str) {
    let s = serde_json::json!({ "t": {
        "list": list,
        "read": "show {ref}",
        "claim": { "bug": "claim {ref}", "report": "claim {ref}", "question": "claim {ref}" },
        "draft": "draft {ref} --note",
        "open": "https://staff.test/i/{ref}" } });
    std::fs::write(d.join(".fael/sources.json"), s.to_string()).unwrap();
}

fn list(d: &Path, lines: &[&str]) {
    std::fs::write(d.join("list.jsonl"), lines.join("\n") + "\n").unwrap();
}

fn line(r: &str, kind: &str, reopen: u32, closed: Option<&str>) -> String {
    serde_json::json!({"ref": r, "kind": kind, "title": format!("{r} title"),
                       "reopen": reopen, "closed": closed})
    .to_string()
}

fn chunks(d: &Path) -> Vec<Value> {
    let v: Value = serde_json::from_str(&fael(d, &["board", "--json"])).unwrap();
    v["projects"][0]["chunks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn by_label(d: &Path, l: &str) -> Vec<Value> {
    chunks(d).into_iter().filter(|c| c["label"] == l).collect()
}

fn brief(d: &Path, uid: &str) -> String {
    let v: Value = serde_json::from_str(&fael(d, &["chunk", "show", uid, "--json"])).unwrap();
    v["brief"].as_str().unwrap().to_string()
}

#[test]
fn tickets_become_one_chunk_each_in_any_plan() {
    let d = repo();
    fael(&d, &["plan", "new", "support", "Support"]);
    // made by an older sync before intake ran, then moved out of inbox
    let old = fael(
        &d,
        &["chunk", "add", "old", "--brief", "b", "--label", "v-0050"],
    );
    let old = old
        .split_whitespace()
        .find(|w| w.len() == 26)
        .unwrap()
        .to_string();
    fael(&d, &["chunk", "move", &old, "--plan", "support"]);
    let twin = fael(
        &d,
        &["chunk", "add", "twin", "--brief", "b", "--label", "v-0050"],
    );
    let twin = twin
        .split_whitespace()
        .find(|w| w.len() == 26)
        .unwrap()
        .to_string();
    let (a, b, c) = (
        line("V-0042", "report", 0, None),
        line("V-0043", "question", 0, None),
        line("V-0044", "feedback", 0, None),
    );
    let (e, gone, r1) = (
        line("V-0050", "bug", 0, None),
        line("V-0051", "bug", 0, Some("dup:V-0040")),
        line("V-0042", "report", 1, None),
    );
    list(&d, &[&a, &b, &c, &e, &gone, &r1]);
    let out = fael(&d, &["intake"]);
    assert!(out.contains("added V-0042 → "), "{out}");
    assert!(out.contains(&format!("adopted V-0050 → {old}")), "{out}");
    assert!(
        out.contains(&format!("not adopted {twin} (V-0050)")),
        "{out}"
    );
    assert!(
        !out.contains("V-0051"),
        "a ticket closed before it came in: {out}"
    );
    assert!(out.ends_with("5 change(s) from 6 ticket(s)\n"), "{out}");

    let r42 = &by_label(&d, "v-0042")[0];
    assert_eq!(
        [&r42["plan"], &r42["state"], &r42["size"]],
        ["inbox", "open", "S"]
    );
    assert_eq!(
        r42["ticket"],
        serde_json::json!({"source": "t", "ref": "V-0042", "kind": "report", "reopen": 0,
                           "open": "https://staff.test/i/V-0042"})
    );
    assert_eq!(by_label(&d, "v-0042-r1")[0]["ticket"]["reopen"], 1);
    let b42 = brief(&d, r42["uid"].as_str().unwrap());
    assert!(
        b42.starts_with("Ticket: V-0042 (t)\nKind: report\nRead: show V-0042\n"),
        "{b42}"
    );
    assert!(
        b42.contains("Claim: claim V-0042 — once you know it is a bug"),
        "{b42}"
    );
    assert!(b42.contains("Draft: draft V-0042 --note \"…\""), "{b42}");
    let q = brief(&d, by_label(&d, "v-0043")[0]["uid"].as_str().unwrap());
    assert!(!q.contains("Claim:"), "a question is never claimed: {q}");
    let f = brief(&d, by_label(&d, "v-0044")[0]["uid"].as_str().unwrap());
    assert!(
        f.contains("Kind: feedback\n") && f.contains("Done: Read it, then:"),
        "{f}"
    );
    assert!(
        !f.contains("Claim:"),
        "feedback is not in the claim object: {f}"
    );
    let adopted: Vec<Value> = by_label(&d, "v-0050");
    let old_c = adopted.iter().find(|c| c["uid"] == old.as_str()).unwrap();
    assert_eq!(
        [&old_c["plan"], &old_c["ticket"]["ref"]],
        ["support", "V-0050"]
    );
    assert!(adopted.iter().find(|c| c["uid"] == twin.as_str()).unwrap()["ticket"].is_null());

    // a rerun adds nothing, and a chunk moved out of inbox is not made again
    fael(
        &d,
        &[
            "chunk",
            "move",
            r42["uid"].as_str().unwrap(),
            "--plan",
            "support",
        ],
    );
    let out = fael(&d, &["intake", "t"]);
    assert_eq!(out, "0 change(s) from 6 ticket(s)\n");
    assert_eq!(by_label(&d, "v-0042").len(), 1);
    let bad = run(&d, &["intake", "nope"]);
    assert!(!bad.status.success());
}

#[test]
fn a_failing_list_writes_nothing() {
    let d = repo();
    let ok = line("V-1", "bug", 0, None);
    for (lines, want) in [
        (
            vec![
                ok.clone(),
                r#"{"ref":"V-2","kind":"bug","title":"t","reopen":0,"closed":null,"email":"x"}"#
                    .into(),
            ],
            "list line 2: field \"email\" is not in the contract",
        ),
        (
            vec![
                ok.clone(),
                r#"{"ref":"V-2","kind":"bug","title":"t","reopen":0}"#.into(),
            ],
            "list line 2: field \"closed\" is missing",
        ),
        (
            vec![ok.clone(), "{not json".into()],
            "list line 2: not JSON",
        ),
        (
            vec![
                ok.clone(),
                r#"{"ref":"V-2","kind":"bug","title":"t","reopen":-1,"closed":null}"#.into(),
            ],
            "list line 2: reopen",
        ),
    ] {
        list(&d, &lines.iter().map(String::as_str).collect::<Vec<_>>());
        let o = run(&d, &["intake"]);
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(1), "{err}");
        assert!(
            err.contains(want) && err.contains("nothing written"),
            "{err}"
        );
    }
    list(&d, &[&ok]);
    sources(&d, "exit 3");
    let err = String::from_utf8_lossy(&run(&d, &["intake"]).stderr).into_owned();
    assert!(err.contains("list exited 3 — nothing written"), "{err}");
    sources(&d, "no-such-fael-adapter list");
    let o = run(&d, &["intake"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1));
    // cmd /C exits 1 with its own "is not recognized" line; sh exits 127
    let want = if cfg!(windows) {
        "list exited 1 — nothing written"
    } else {
        "list: command not found — no-such-fael-adapter list"
    };
    assert!(err.contains(want), "{err}");
    // nothing was ever written: no db, or one with no chunk
    assert!(run(&d, &["board", "--json"]).stdout.is_empty() || chunks(&d).is_empty());
}

#[test]
fn two_intakes_at_once_make_one_chunk_per_ticket() {
    let d = repo();
    let lines: Vec<String> = (0..40)
        .map(|i| line(&format!("V-{i}"), "bug", 0, None))
        .collect();
    list(&d, &lines.iter().map(String::as_str).collect::<Vec<_>>());
    let spawn = || {
        Command::new(env!("CARGO_BIN_EXE_fael"))
            .arg("intake")
            .current_dir(&d)
            .env("FAEL_STATE_DIR", d.join("state"))
            .env_remove("FAEL_DIR")
            .output()
    };
    let (x, y) = std::thread::scope(|s| {
        let x = s.spawn(spawn);
        let y = s.spawn(spawn);
        (x.join().unwrap().unwrap(), y.join().unwrap().unwrap())
    });
    assert!(x.status.success() && y.status.success());
    let outs = [x, y].map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    assert_eq!(
        outs.iter()
            .map(|o| o.matches("added ").count())
            .sum::<usize>(),
        40,
        "{outs:?}"
    );
    assert_eq!(chunks(&d).len(), 40);
}
