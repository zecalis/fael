//! `doctor [Superseded]`: a legacy supersede chain hidden by a close of its
//! newest version. A pre-chain-close binary closed only the newest version, so
//! the old versions stayed hidden with no close row and no command reached
//! them — the note names them, `fael close <id>` repairs each. Split out of
//! `doctor.rs` for the 400-line cap.

use super::{fael, repo};

#[test]
fn doctor_flags_a_chain_hidden_by_a_closed_head() {
    let d = repo();
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, out, err) = fael(&d, &["add", "issue", "hot", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let a = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(&d, &["bump", &a, "--to", "ploy"]);
    assert!(ok, "{err}");
    let b = out.split_whitespace().next().unwrap().to_string();
    // the legacy trap: a pre-chain-close binary appended only B's close row,
    // so A sits hidden with no close row — write that close row raw, the way
    // the old binary did, since `fael close` now closes the whole chain
    let writer = std::fs::read_dir(d.join(".fael/log"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .unwrap();
    let month = std::fs::read_dir(&writer)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with(".jsonl"))
        .unwrap();
    let close_file = month.with_extension("close.jsonl");
    std::fs::write(
        &close_file,
        format!(
            "{{\"v\":1,\"id\":\"01M3NS0000000000000000000C\",\"ts\":\"2026-09-01T00:00:00.000Z\",\
             \"by\":\"tester-0000\",\"text\":\"done\",\"ref\":\"{b}\"}}\n"
        ),
    )
    .unwrap();
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok, "{out}");
    assert!(out.contains("note [Superseded]"), "{out}");
    // `--json` carries the full id a cleanup agent closes
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps: Vec<serde_json::Value> = serde_json::from_str(out.trim()).unwrap();
    let sup = ps
        .iter()
        .find(|p| p["kind"] == "superseded")
        .expect("superseded");
    assert_eq!(sup["ids"], serde_json::json!([a]), "{sup}");
    // the fix is a plain `fael close` on the old id — now allowed
    let (ok, _, err) = fael(&d, &["close", &a, "superseded"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Superseded]"), "{out}");
}
