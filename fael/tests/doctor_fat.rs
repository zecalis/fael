//! Chunk 10 through the real binary: doctor [Fat] repeats the add-time
//! warnings the agent skipped — open rows only, closed rows never count.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-fat-cli-{}", fael_core::ulid()));
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
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

#[test]
fn doctor_flags_fat_rows_and_ignores_closed_ones() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    // the plan's done criterion: open decision, no key, three separators
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "a; b; c; d",
            "--files",
            "src/a.rs",
            "--force",
        ],
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Fat]: 1 open row(s)") && out.contains(&id[..8]),
        "{out}"
    );
    // a keyed single-topic decision next to it stays silent
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "single topic",
            "--key",
            "a:b",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Fat]: 1 open row(s)") && out.contains(&id[..8]),
        "{out}"
    );
    // closing the fat row clears it — closed rows never count
    let (ok, _, err) = fael(&d, &["close", &id[..12], "split done"]);
    assert!(ok, "{err}");
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok && !out.contains("[Fat]"), "{out}");
}

#[test]
fn doctor_collapses_legacy_fat_rows_until_fat_flag() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    // one new fat row through the real write path
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "a; b; c; d",
            "--files",
            "src/a.rs",
            "--force",
        ],
    );
    assert!(ok, "{err}");
    let new_id = out.split_whitespace().next().unwrap().to_string();
    // one pre-self-heal fat row: a 2023 ULID appended straight to the tree
    // log (the journal copy never exists, so the union still sees it once)
    let wdir = std::fs::read_dir(d.join(".fael/log"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let logs: Vec<PathBuf> = std::fs::read_dir(&wdir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(logs.len(), 1);
    let legacy_id = fael_core::ulid_at(1_700_000_000_000);
    let mut s = std::fs::read_to_string(&logs[0]).unwrap();
    s.push_str(&format!(
        "{{\"v\":1,\"id\":\"{legacy_id}\",\"ts\":\"2023-11-14T22:13:20Z\",\"by\":\"t-0000\",\
         \"kind\":\"decision\",\"text\":\"legacy; fat; row; here\",\"files\":[\"src/a.rs\"],\
         \"fh\":{{\"src/a.rs\":\"e69de29bb2d1\"}}}}\n" // stamped, so [Unstamped] stays out of this test
    ));
    std::fs::write(&logs[0], s).unwrap();
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    // default: the new row lists, the legacy row collapses to one line
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok, "{out}");
    assert!(
        out.contains(&new_id[..8])
            && out.contains("1 legacy rows — fael doctor --fat --json")
            && !out.contains(&legacy_id[..8]),
        "{out}"
    );
    // --fat: the one-time pass lists every fat row, legacy included
    let (ok, out, _) = fael(&d, &["doctor", "--fat"]);
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Fat]: 2 open row(s)")
            && out.contains(&new_id[..8])
            && out.contains(&legacy_id[..8]),
        "{out}"
    );
    // --fat --json stays pure JSON and still expands the legacy row
    let (ok, out, _) = fael(&d, &["doctor", "--fat", "--json"]);
    assert!(ok, "{out}");
    assert!(
        out.trim_start().starts_with('[')
            && out.contains(&legacy_id[..8])
            && out.contains(&new_id[..8]),
        "{out}"
    );
}
