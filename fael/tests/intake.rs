//! `fael intake` (plan ledger-intake) through the real binary with a fixture adapter whose
//! `list` prints a JSONL file: a ticket round is one ledger row on `ticket:<source>:<ref>`,
//! a run twice writes it once, the source closing it closes the row (by `intake`), and every
//! failing list writes nothing.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(d: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", d.join("../state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// A git repo whose source `vela` lists `list.jsonl` (`type` under cmd /C on Windows).
fn repo() -> PathBuf {
    let base = std::env::temp_dir().join(format!("fael-intake-{}", fael_core::ulid()));
    let d = base.join("r");
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "Test User"],
        &["config", "user.email", "t@example.com"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&d)
                .status()
                .unwrap()
                .success()
        );
    }
    let list = if cfg!(windows) {
        "type list.jsonl"
    } else {
        "cat list.jsonl"
    };
    let s = serde_json::json!({ "vela": {
        "list": list,
        "read": "show {ref}",
        "draft": "draft {ref}",
        "claim": "claim {ref}",
        "open": "https://staff.test/i/{ref}" } });
    std::fs::write(d.join(".fael/sources.json"), s.to_string()).unwrap();
    d
}

fn list(d: &Path, lines: &[String]) {
    std::fs::write(d.join("list.jsonl"), lines.join("\n") + "\n").unwrap();
}

fn line(r: &str, kind: &str, reopen: u32, closed: Option<&str>) -> String {
    serde_json::json!({"ref": r, "kind": kind, "title": format!("{r} title"),
                       "reopen": reopen, "closed": closed})
    .to_string()
}

/// Every ticket row ever filed (open or closed), as `find --json` prints them.
fn rows(d: &Path) -> Vec<Value> {
    let (ok, out, err) = fael(d, &["find", "--json", "--all", "--key", "ticket:*"]);
    assert!(ok, "{err}");
    out.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn three(d: &Path) {
    list(
        d,
        &[
            line("V-1", "bug", 0, None),
            line("V-2", "report", 0, None),
            line("V-3", "idea", 0, None),
        ],
    );
}

#[test]
fn intake_three_tickets_three_rows_and_a_second_run_writes_nothing() {
    let d = repo();
    three(&d);
    let (ok, out, err) = fael(&d, &["intake"]);
    assert!(ok, "{err}");
    assert!(out.ends_with("3 change(s) from 3 ticket(s)\n"), "{out}");
    let r = rows(&d);
    assert_eq!(r.len(), 3, "{r:?}");
    let by = |k: &str| r.iter().find(|x| x["key"] == k).unwrap();
    let v1 = by("ticket:vela:v-1");
    assert_eq!(v1["kind"], "issue");
    assert_eq!(v1["by"], "intake");
    assert_eq!(v1["title"], "V-1 title");
    assert_eq!(v1["files"][0], "ticket:vela:v-1");
    assert!(v1["text"].as_str().unwrap().contains("Read: show V-1."));
    assert!(
        v1["text"]
            .as_str()
            .unwrap()
            .contains("Open: https://staff.test/i/V-1.")
    );
    assert_eq!(by("ticket:vela:v-2")["kind"], "issue");
    assert_eq!(by("ticket:vela:v-3")["kind"], "note");

    let (ok, out, err) = fael(&d, &["intake"]);
    assert!(ok, "{err}");
    assert!(out.ends_with("0 change(s) from 3 ticket(s)\n"), "{out}");
    assert_eq!(rows(&d), r, "a second run leaves the rows as they were");
}

#[test]
fn intake_the_source_closing_a_ticket_closes_its_row_by_intake() {
    let d = repo();
    three(&d);
    assert!(fael(&d, &["intake"]).0);
    list(
        &d,
        &[
            line("V-1", "bug", 0, Some("closed")),
            line("V-2", "report", 0, Some("dup:V-1")),
            line("V-3", "idea", 0, None),
        ],
    );
    let (ok, out, err) = fael(&d, &["intake"]);
    assert!(ok, "{err}");
    assert!(out.ends_with("2 change(s) from 3 ticket(s)\n"), "{out}");
    let (_, open, _) = fael(&d, &["find", "--json", "--key", "ticket:*"]);
    assert_eq!(open.lines().count(), 1, "only V-3 stays open: {open}");
    let (_, all, _) = fael(&d, &["find", "--json", "--all", "--key", "ticket:vela:v-2"]);
    assert!(all.contains("closed at source: dup:V-1"), "{all}");
    // a third run: nothing to close, nothing reopened
    let (_, out, _) = fael(&d, &["intake"]);
    assert!(out.starts_with("0 change(s)"), "{out}");
}

#[test]
fn intake_a_ticket_an_agent_closed_is_not_reopened_while_the_source_lists_it() {
    let d = repo();
    three(&d);
    assert!(fael(&d, &["intake"]).0);
    let id = rows(&d)[0]["id"].as_str().unwrap().to_string();
    assert!(
        fael(
            &d,
            &[
                "close",
                &id,
                "ticket fixed → shipped; guard `src/v_test.rs`"
            ]
        )
        .0
    );
    let (ok, out, _) = fael(&d, &["intake"]);
    assert!(ok);
    assert!(out.starts_with("0 change(s)"), "{out}");
    let filed = rows(&d).into_iter().filter(|r| r["ref"].is_null()).count();
    assert_eq!(filed, 3);
}

#[test]
fn intake_a_reopened_round_is_its_own_row_and_says_where_the_round_before_stopped() {
    let d = repo();
    list(&d, &[line("V-1", "bug", 0, None)]);
    assert!(fael(&d, &["intake"]).0);
    assert!(
        fael(
            &d,
            &[
                "close",
                "--key",
                "ticket:vela:v-1",
                "ticket fixed → shipped; guard `src/v_test.rs`",
            ]
        )
        .0
    );
    list(
        &d,
        &[line("V-1", "bug", 0, None), line("V-1", "bug", 1, None)],
    );
    let (ok, out, err) = fael(&d, &["intake"]);
    assert!(ok, "{err}");
    assert!(out.starts_with("added V-1 → "), "{out}");
    let (_, one, _) = fael(&d, &["find", "--json", "--key", "ticket:vela:v-1-r1"]);
    let text = serde_json::from_str::<Value>(one.lines().next().unwrap()).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        text.contains("round 0 stopped — closed: ticket fixed → shipped"),
        "{text}"
    );
}

fn open_rows(d: &Path) -> Vec<Value> {
    let all = rows(d);
    let shut: Vec<&Value> = all.iter().map(|r| &r["ref"]).collect();
    let open = |r: &&Value| r["ref"].is_null() && !shut.contains(&&r["id"]);
    all.iter().filter(open).cloned().collect()
}

#[test]
fn intake_a_reopen_closes_the_round_still_open() {
    let d = repo();
    list(&d, &[line("V-1", "bug", 0, None)]);
    assert!(fael(&d, &["intake"]).0);
    list(&d, &[line("V-1", "bug", 1, None)]);
    let (ok, out, err) = fael(&d, &["intake"]);
    assert!(ok, "{err}");
    assert!(out.starts_with("closed V-1 → "), "{out}");
    let (_, one, _) = fael(&d, &["find", "--json", "--all", "--key", "ticket:vela:v-1"]);
    assert!(one.contains("reopened at source as round 1"), "{one}");
    let open = open_rows(&d);
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0]["key"], "ticket:vela:v-1-r1");
}

#[test]
fn intake_closes_the_newer_of_two_open_rows_on_one_key() {
    let d = repo();
    list(&d, &[line("V-1", "bug", 0, None)]);
    assert!(fael(&d, &["intake"]).0);
    // what a sync of another machine's intake leaves behind
    let key = "ticket:vela:v-1";
    assert!(
        fael(
            &d,
            &[
                "add", "issue", "copy", "--title", "copy", "--files", key, "--key", key
            ]
        )
        .0
    );
    let mut ids: Vec<Value> = open_rows(&d).iter().map(|r| r["id"].clone()).collect();
    ids.sort_by_key(|i| i.as_str().unwrap().to_string());
    let (older, newer) = (&ids[0], &ids[1]);
    let (ok, out, err) = fael(&d, &["intake"]);
    assert!(ok, "{err}");
    assert!(out.starts_with("closed V-1 → "), "{out}");
    let (_, closed, _) = fael(&d, &["find", "--json", "--all", "--key", key]);
    assert!(
        closed.contains(&format!("duplicate of {}", older.as_str().unwrap())),
        "{closed}"
    );
    let open = open_rows(&d);
    assert_eq!(open.len(), 1, "{open:?} {newer}");
    assert_eq!(&open[0]["id"], older);
    let (_, again, _) = fael(&d, &["intake"]);
    assert!(again.starts_with("0 change(s)"), "{again}");
}

#[test]
fn intake_a_bad_list_writes_nothing_and_exits_one() {
    for bad in [
        // a field outside the contract (customer text)
        r#"{"ref":"V-9","kind":"bug","title":"t","reopen":0,"closed":null,"body":"hi"}"#,
        r#"{"ref":"V-9","kind":"bug","title":"t","reopen":0}"#,
        r#"{"ref":"V-9","kind":"bug","title":"t","reopen":0,"closed":"maybe"}"#,
        "not json",
        // a kind with no row to write
        r#"{"ref":"V-9","kind":"feedback","title":"t","reopen":0,"closed":null}"#,
        r#"{"ref":"V-9","kind":"feedback","title":"t","reopen":0,"closed":"closed"}"#,
    ] {
        let d = repo();
        list(&d, &[line("V-1", "bug", 0, None), bad.to_string()]);
        let (ok, _, err) = fael(&d, &["intake"]);
        assert!(!ok, "{bad}");
        assert!(err.contains("nothing written"), "{err}");
        if bad.contains("feedback") {
            assert!(err.contains("feedback"), "{err}");
        }
        assert!(rows(&d).is_empty(), "{bad}: V-1 must not be written either");
    }
}

#[test]
fn intake_a_failing_list_command_writes_nothing() {
    let d = repo();
    let s = serde_json::json!({ "vela": { "list": "exit 3" }});
    std::fs::write(d.join(".fael/sources.json"), s.to_string()).unwrap();
    let (ok, _, err) = fael(&d, &["intake"]);
    assert!(!ok);
    assert!(err.contains("list exited 3"), "{err}");
    assert!(rows(&d).is_empty());
}

#[test]
fn intake_a_named_source_and_a_missing_file_say_what_is_wrong() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["intake", "nope"]);
    assert!(!ok);
    assert!(
        err.contains("no source nope in .fael/sources.json — vela"),
        "{err}"
    );
    std::fs::remove_file(d.join(".fael/sources.json")).unwrap();
    let (ok, _, err) = fael(&d, &["intake"]);
    assert!(!ok);
    assert!(err.contains("no .fael/sources.json"), "{err}");
}

#[test]
fn intake_a_ticket_closed_twice_in_one_list_closes_its_row_once() {
    let d = repo();
    list(&d, &[line("A1", "bug", 0, None)]);
    assert!(fael(&d, &["intake"]).0);
    list(
        &d,
        &[
            line("A1", "bug", 0, Some("closed")),
            line("a1", "bug", 0, Some("dup:A9")),
        ],
    );
    let (ok, out, err) = fael(&d, &["intake"]);
    assert!(ok, "{err}");
    assert!(out.ends_with("1 change(s) from 2 ticket(s)\n"), "{out}");
    assert!(!err.contains("new key"), "{err}");
}
