//! PLAN-fael-agent-ergonomics chunk 2: what an agent guesses runs like the real
//! command, and a guess that cannot is rejected with the closest real thing.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-syn-{}", fael_core::ulid()));
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

/// The id `fael add` receipted — its first 26 chars.
fn add(d: &Path, args: &[&str]) -> String {
    let (ok, out, err) = fael(d, args);
    assert!(ok, "{args:?} {err}");
    out[..26].to_string()
}

#[test]
fn a_guess_runs_like_the_real_command() {
    let d = repo();
    let id = add(&d, &["add", "issue", "login loops", "--files", "src/a.rs"]);
    add(
        &d,
        &["add", "note", "timeout is 30s", "--files", "src/b.rs"],
    );
    for (guess, real) in [
        (
            &["find", "-n", "3", "--type", "issue"][..],
            &["find", "--limit", "3", "--kind", "issue"][..],
        ),
        (
            &["find", "--query", "timeout"],
            &["find", "--text", "timeout"],
        ),
        (&["find", "-q", "timeout"], &["find", "--text", "timeout"]),
        (
            &["find", "--file", "src/a.rs"],
            &["find", "--files", "src/a.rs"],
        ),
        (&["find", "-a", "-j"], &["find", "--all", "--json"]),
        (&["find", "--id", &id], &["find", &id]),
        (&["show", &id], &["find", &id]),
        (&["list", "--type", "issue"], &["find", "--kind", "issue"]),
    ] {
        let (a, b) = (fael(&d, guess), fael(&d, real));
        assert!(a.0 && b.0, "{guess:?} {a:?}");
        assert_eq!(a, b, "{guess:?} vs {real:?}");
    }
    // `fael decision "…" --file a.rs` files the same row as `fael add decision …`
    let g = add(
        &d,
        &[
            "decision",
            "use tokio",
            "--file",
            "src/c.rs",
            "--tag",
            "rt:pick",
        ],
    );
    let (_, out, _) = fael(&d, &["find", &g]);
    assert!(
        out.contains("decision") && out.contains("src/c.rs") && out.contains("rt:pick"),
        "{out}"
    );
    // close: every spelling of the reason, wherever it stands
    let (ok, _, err) = fael(&d, &["close", &id, "--why", "fixed in a1b2c3"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["find", &id, "--all"]);
    assert!(out.contains("fixed in a1b2c3"), "{out}");
    let n = add(&d, &["add", "note", "-m", "via -m", "--files", "src/d.rs"]);
    let (ok, _, err) = fael(&d, &["done", "--reason", "done", &n]);
    assert!(ok, "{err}");
}

#[test]
fn a_guess_that_cannot_run_names_the_closest_and_the_usage() {
    let d = repo();
    let id = add(&d, &["add", "issue", "login loops", "--files", "src/a.rs"]);
    for (args, said) in [
        (
            &["find", "--stale"][..],
            "unknown flag --stale — open issues: fael find --kind issue",
        ),
        (
            &["find", "-k", "auth"],
            "unknown flag -k — did you mean --key or --kind?",
        ),
        (
            &["find", "--group"],
            "unknown flag --group — did you mean --groups?",
        ),
        (
            &["find", "--status", "open"],
            "unknown flag --status — --all adds closed rows",
        ),
        (
            &["bump", &id, "--title", "x"],
            "bump takes no --title — re-file it: fael add … --supersedes <id>",
        ),
        (&["close", &id, "x", "--to", "y"], "close takes no --to"),
        (&["fnid"], "unknown command \"fnid\" — did you mean find?"),
    ] {
        let (ok, out, err) = fael(&d, args);
        assert!(!ok && out.is_empty(), "{args:?} {out}");
        assert!(
            err.starts_with("rejected: ") && err.contains(said),
            "{args:?} {err}"
        );
        assert!(!err.contains("not an id"), "{args:?} {err}");
        if args[0] != "fnid" {
            assert!(err.contains("\nusage: fael "), "{args:?} {err}");
        }
    }
    // after `--` a dash word is text, and `-` alone is still stdin for a batch
    let (ok, _, err) = fael(&d, &["add", "note", "--files", "src/a.rs", "--", "-k"]);
    assert!(ok, "{err}");
}

fn mcp(d: &Path, tool: &str, args: serde_json::Value) -> serde_json::Value {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", d.join("state"))
        .current_dir(d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": args}});
    writeln!(c.stdin.take().unwrap(), "{call}").unwrap();
    let out = c.wait_with_output().unwrap().stdout;
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn mcp_properties_are_read_like_the_real_ones() {
    let d = repo();
    let id = add(&d, &["add", "issue", "login loops", "--files", "src/a.rs"]);
    let r = mcp(&d, "find", serde_json::json!({"query": "login"}));
    assert!(
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("login loops"),
        "{r}"
    );
    let r = mcp(
        &d,
        "close",
        serde_json::json!({"id": id, "why": "fixed in a1b2c3"}),
    );
    assert_eq!(r["result"]["isError"], false, "{r}");
    let (_, out, _) = fael(&d, &["find", &id, "--all"]);
    assert!(out.contains("fixed in a1b2c3"), "{out}");
}
