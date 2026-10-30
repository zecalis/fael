//! `doctor --fix` mechanical tiers (plan fael-secretary c6): auto-fix only
//! mechanically provable state — a stuck `[Superseded]` chain, `[Gone]` rows,
//! broken/torn content — and report everything else with its ready
//! `fael close` command. Second run is a no-op; ambiguous dupes are never
//! canon-picked and unstamped rows never auto-bumped.

use super::{fael, repo};
use std::path::{Path, PathBuf};

/// stdout's first token of `fael add`: `<id> → <path>`.
fn first_id(out: &str) -> String {
    out.split_whitespace().next().unwrap().to_string()
}

/// The writer's current month file under the journal (`.git/fael/log`).
fn writer_month(d: &Path) -> PathBuf {
    let writer = std::fs::read_dir(d.join(".git/fael/log"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .unwrap();
    std::fs::read_dir(&writer)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.to_string_lossy().ends_with(".jsonl")
                && !p.to_string_lossy().ends_with(".close.jsonl")
        })
        .unwrap()
}

fn kinds(out: &str) -> Vec<serde_json::Value> {
    serde_json::from_str(out.trim()).unwrap()
}

/// `--fix` closes a stuck superseded chain and all-gone rows with zero human
/// picks; a second `doctor --json` shows those classes empty and a second
/// `--fix` prints no `fixed:` line.
#[test]
fn fix_closes_superseded_and_gone_second_run_noop() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, out, err) = fael(&d, &["add", "issue", "hot", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let a = first_id(&out);
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "hot, re-filed",
            "--files",
            "src/a.rs",
            "--supersedes",
            &a,
        ],
    );
    assert!(ok, "{err}");
    let b = first_id(&out);
    // the legacy trap: a pre-chain-close binary appended only B's close row,
    // so A sits hidden with no close row — written raw the way the old binary
    // did, since `fael close` now closes the whole chain
    let month = writer_month(&d);
    std::fs::write(
        month.with_extension("close.jsonl"),
        format!(
            "{{\"v\":1,\"id\":\"01M3NS0000000000000000000C\",\"ts\":\"2026-09-01T00:00:00.000Z\",\
              \"by\":\"tester-0000\",\"text\":\"done\",\"ref\":\"{b}\"}}\n"
        ),
    )
    .unwrap();
    // an all-gone row: its files never existed
    let (ok, out, err) = fael(
        &d,
        &["add", "note", "dead pointer", "--files", "src/missing.rs"],
    );
    assert!(ok, "{err}");
    let c = first_id(&out);
    // report: both listed, each with its ready command
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Superseded] [--fix]")
            && out.contains(&format!("fael close {}", &a[..8])),
        "{out}"
    );
    assert!(
        out.contains("note [Gone] [--fix]") && out.contains(&format!("fael close {}", &c[..8])),
        "{out}"
    );
    // --fix closes both, naming each action (the printed id is the new close
    // row, as with [Shipped]; the targets are pinned below via find/--json)
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    assert!(out.contains("— superseded"), "{out}");
    assert!(out.contains("— gone"), "{out}");
    for id in [&a, &c] {
        let (ok, out, _) = fael(&d, &["find", id]);
        assert!(ok && out.contains("closed:"), "{id} {out}");
    }
    // those classes are empty now
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps = kinds(&out);
    assert!(
        !ps.iter()
            .any(|p| p["kind"] == "superseded" || p["kind"] == "gone"),
        "{ps:?}"
    );
    // and a second --fix changes nothing
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    assert!(!out.contains("fixed: closed"), "{out}");
}

/// Ambiguous tiers stay report-only: PartGone + Stale are listed each with a
/// ready `fael close` command and full ids, and `--fix` closes nothing.
#[test]
fn ambiguous_tiers_are_report_only_with_ready_commands() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "half gone",
            "--files",
            "src/a.rs,src/removed.rs",
            "--force",
        ],
    );
    assert!(ok, "{err}");
    std::fs::write(d.join("keep.yaml"), "").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "catalog in `keep.yaml`",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    std::fs::remove_file(d.join("keep.yaml")).unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [PartGone]") && out.contains("fael close"),
        "{out}"
    );
    assert!(
        out.contains("note [Stale]") && out.contains("fael close"),
        "{out}"
    );
    // nothing mechanical here: --fix prints no fixed: line and closes nothing
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    assert!(!out.contains("fixed:"), "{out}");
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps = kinds(&out);
    for kind in ["partgone", "stale"] {
        let p = ps.iter().find(|p| p["kind"] == kind).expect(kind);
        assert_eq!(p["fixable"], serde_json::json!(false), "{p}");
        assert!(!p["ids"].as_array().unwrap().is_empty(), "{p}");
        assert!(p["detail"].as_str().unwrap().contains("fael close"), "{p}");
    }
}

/// `--fix` never auto-bumps: an unstamped row keeps its bytes and its note.
#[test]
fn fix_never_auto_bumps_unstamped_rows() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &["add", "decision", "later file", "--files", "src/later.rs"],
    );
    assert!(ok, "{err}");
    let id = first_id(&out);
    std::fs::write(d.join("src/later.rs"), "fn main() {}\n").unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Unstamped]") && out.contains("fael bump"),
        "{out}"
    );
    let month = writer_month(&d);
    let before = std::fs::read(&month).unwrap();
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    assert!(!out.contains("fixed:"), "{out}");
    assert_eq!(std::fs::read(&month).unwrap(), before, "row restamped");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Unstamped]") && out.contains(&id[..8]),
        "{out}"
    );
}

/// Corruption and logical duplication are separate fixtures: a torn tail is
/// quarantined (bytes preserved, never repaired by guessing) while the
/// storage duplicate beside it stays reported — and a second `--fix` still
/// reports it.
#[test]
fn torn_storage_quarantined_while_duplicate_stays_reported() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, out, err) = fael(&d, &["add", "note", "keeper", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let id = first_id(&out);
    let month = writer_month(&d);
    // same id twice (partial-write double append) plus an unterminated tail
    let mut body = std::fs::read_to_string(&month).unwrap();
    let line = body.lines().next().unwrap().to_string();
    body.push_str(&line);
    body.push('\n');
    body.push_str("{\"v\":1,\"id\":\"TOR");
    std::fs::write(&month, &body).unwrap();
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("appears 2×") && out.contains("read keeps the first"),
        "{out}"
    );
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(!ok, "{out}");
    // the torn bytes moved to quarantine, the good lines stayed byte-identical
    let qdir = d.join(".git/fael/quarantine");
    let qs: Vec<PathBuf> = std::fs::read_dir(&qdir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(qs.len(), 1, "{qdir:?}");
    assert!(
        std::fs::read_to_string(&qs[0]).unwrap().contains("TOR"),
        "{:?}",
        qs[0]
    );
    assert_eq!(
        std::fs::read_to_string(&month).unwrap(),
        format!("{line}\n{line}\n")
    );
    // the duplicate is still reported (error), never closed or repaired
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(out.contains("appears 2×"), "{out}");
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let dupe = kinds(&out)
        .into_iter()
        .find(|p| p["kind"] == "duplicate")
        .expect("duplicate");
    assert_eq!(dupe["fixable"], serde_json::json!(false));
    assert_eq!(dupe["ids"], serde_json::json!([id]));
    assert!(
        dupe["detail"]
            .as_str()
            .unwrap()
            .contains("read keeps the first")
    );
    // the logical row itself still reads (first-wins)
    let (ok, _, _) = fael(&d, &["find", &id]);
    assert!(ok);
}

/// Two distinct ids on the same files are a judgement call, never a `--fix`:
/// both rows stay open, no `fixed:` line, no `Duplicate` (different ids).
#[test]
fn logical_duplicates_are_never_closed_by_fix() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "timeout in auth login",
            "--key",
            "t:login",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let first = first_id(&out);
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "timeout in auth logout",
            "--key",
            "t:logout",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let second = first_id(&out);
    assert_ne!(first, second);
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    assert!(!out.contains("fixed: closed"), "{out}");
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps = kinds(&out);
    assert!(!ps.iter().any(|p| p["kind"] == "duplicate"), "{ps:?}");
    for id in [&first, &second] {
        let (ok, out, _) = fael(&d, &["find", id]);
        assert!(ok && !out.contains("closed:"), "{id} {out}");
    }
}
