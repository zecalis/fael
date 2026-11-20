//! `fael bump` + urgent over MCP: the binary in a throwaway git repo.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"));
    let o = c.output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-bump-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
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
    d
}

#[test]
fn bump_rejects_a_flag_it_never_reads() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["add", "issue", "a", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(&d, &["bump", &id, "--dry-run"]);
    assert!(!ok && err.contains("bump takes no --dry-run"), "{err}");
    let (_, out, _) = fael(&d, &["find", &id]);
    assert!(!out.contains("superseded"), "bump wrote a version:\n{out}");
    let (ok, _, err) = fael(&d, &["bump", &id, "--to", "Ploy"]);
    assert!(!ok && err.contains("bump takes no --to"), "{err}");
}

#[test]
fn close_and_mv_reject_a_flag_they_never_read() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["add", "issue", "a", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(&d, &["close", &id, "done", "--dry-run"]);
    assert!(!ok && err.contains("close takes no --dry-run"), "{err}");
    let (ok, _, err) = fael(&d, &["mv", "src/a.rs", "src/b.rs", "--force"]);
    assert!(!ok && err.contains("mv takes no --force"), "{err}");
    // a shapeless close rejects before the write; --force files it as is
    let (ok, _, err) = fael(&d, &["close", &id, "done"]);
    assert!(!ok && err.contains("close names no fix"), "{err}");
    let (ok, _, err) = fael(&d, &["close", &id, "done", "--force"]);
    assert!(ok, "close --force was rejected: {err}");
}

/// A bump or claim moves the row in place: it never says `supersedes` (the
/// edge the row was filed with is not something this call replaced), and
/// only a bump that routes it repeats the receiver lines.
#[test]
fn bump_and_claim_never_say_the_filed_supersede() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["add", "issue", "hot", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let a = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "hot, v2",
            "--files",
            "src/a.rs",
            "--supersedes",
            &a,
        ],
    );
    assert!(ok, "{err}");
    let b = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(&d, &["bump", &b]);
    assert!(ok, "{err}");
    assert!(out.starts_with(&b) && !out.contains("supersedes"), "{out}");
    for args in [&["bump", &b][..], &["claim", &b]] {
        let (ok, out, err) = fael(&d, args);
        assert!(ok, "{args:?}: {err}");
        assert!(out.starts_with(&b) && !out.contains("supersedes"), "{out}");
        assert!(
            !out.contains("tell them") && !out.contains("start it:"),
            "{args:?} is not a hand-off: {out}"
        );
    }
}

/// `find <id>` pulls a content row, never a carrier: a bump event sharing
/// the short id's prefix (filed right after its row) must not turn the pull
/// into a text search (CLI) or an ambiguity error (MCP).
/// A bump event for `id` whose id shares its first 25 chars — what a bump
/// in the same ms as the add would write — appended to the journal.
fn plant_event(d: &Path, id: &str) {
    use std::io::Write;
    let (_, json, _) = fael(d, &["find", id, "--json"]);
    let row: serde_json::Value = serde_json::from_str(json.lines().next().unwrap()).unwrap();
    let last = if id.ends_with('0') { "1" } else { "0" };
    let ev = serde_json::json!({
        "v": 1, "id": format!("{}{last}", &id[..25]), "ts": row["ts"],
        "by": row["by"], "text": format!("{id} bumped"), "bumps": id,
    });
    let log = std::fs::read_dir(d.join(".git/fael/log"))
        .unwrap()
        .flatten()
        .flat_map(|w| std::fs::read_dir(w.path()).unwrap().flatten())
        .map(|e| e.path())
        .find(|p| {
            p.to_string_lossy().ends_with(".jsonl") && !p.to_string_lossy().contains(".close.")
        })
        .unwrap();
    let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    writeln!(f, "{ev}").unwrap();
}

#[test]
fn a_supersedes_flag_sharing_a_bump_events_prefix_is_never_rescued_elsewhere() {
    // self-heal checks the flag with the same lookup add uses: a short id
    // that also prefixes a bump event still names its row, so the text's
    // mention of another open row never redirects the edge
    let d = repo();
    let id_of = |out: &str| out.split_whitespace().next().unwrap().to_string();
    let (ok, a, err) = fael(&d, &["add", "issue", "pull me", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let (ok, b, err) = fael(&d, &["add", "issue", "other", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let (a, b) = (id_of(&a), id_of(&b));
    plant_event(&d, &a);
    let text = format!("v2, supersedes {b}");
    let args = [
        "add",
        "issue",
        &text,
        "--supersedes",
        &a[..25],
        "--files",
        "src/a.rs",
    ];
    let (ok, out, err) = fael(&d, &args);
    assert!(ok, "{err}");
    assert!(out.contains(&format!("supersedes {a}")), "{out}{err}");
}

#[test]
fn find_by_short_id_looks_past_a_bump_event_sharing_the_prefix() {
    use std::io::Write;
    let d = repo();
    let (ok, out, err) = fael(&d, &["add", "issue", "pull me", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    plant_event(&d, &id);
    let short = &id[..25];
    let (ok, out, err) = fael(&d, &["find", short]);
    assert!(ok && out.contains("pull me"), "{out}{err}");
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"find","arguments":{"id":short}}});
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", d.join("state"))
        .current_dir(&d)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(format!("{req}\n").as_bytes())
        .unwrap();
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    let r: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(r["result"]["isError"], false, "{out}");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("pull me"), "{text}");
}
