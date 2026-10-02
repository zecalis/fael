//! `find` that saves the agent a round: several ids in one call, a short list
//! that shows its bodies, an empty search that says which word matched
//! nothing — and records the miss for `fael stats --misses`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root(dir: &Path) -> &Path {
    dir.ancestors().find(|p| p.join(".git").exists()).unwrap()
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", root(dir).join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-find-batch-{}", fael_core::ulid()));
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
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

/// A note on its own file, titled so lists and bodies read differently.
fn add(d: &Path, name: &str, title: &str, body: &str) -> String {
    std::fs::write(d.join("src").join(name), "// x\n").unwrap();
    let (ok, out, err) = fael(
        d,
        &[
            "add",
            "note",
            body,
            "--title",
            title,
            "--files",
            &format!("src/{name}"),
        ],
    );
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

#[test]
fn several_ids_print_every_body_and_a_bad_one_fails_alone() {
    let d = repo();
    let a = add(&d, "a.rs", "first headline", "first body words");
    let b = add(&d, "b.rs", "second headline", "second body words");
    let (ok, out, err) = fael(&d, &["find", &a, &b]);
    assert!(ok, "{err}");
    assert!(
        out.contains("first body words") && out.contains("second body words"),
        "{out}"
    );
    // a bad id is named, the good one still prints, the exit says one failed
    let (ok, out, err) = fael(&d, &["find", &a, "01DEFACED01"]);
    assert!(!ok);
    assert!(
        out.contains("first body words") && out.contains("rejected: no row with id"),
        "{out}"
    );
    assert!(err.contains("1 of 2 ids not shown"), "{err}");
    // two words are a text query's job, not an id list's
    let (ok, out, err) = fael(&d, &["find", &a, "not-an-id"]);
    assert!(!ok && out.contains("is not an id"), "{out}{err}");
}

#[test]
fn a_short_list_shows_bodies_and_a_long_one_stays_titles() {
    let d = repo();
    add(&d, "a.rs", "alpha headline", "alpha body words");
    add(&d, "b.rs", "beta headline", "beta body words");
    let (_, out, _) = fael(&d, &["find", "--kind", "note"]);
    assert!(
        out.contains("alpha body words") && out.contains("beta body words"),
        "{out}"
    );
    add(&d, "c.rs", "gamma headline", "gamma body words");
    let (_, out, _) = fael(&d, &["find", "--kind", "note"]);
    assert!(
        out.contains("gamma headline") && !out.contains("gamma body words"),
        "{out}"
    );
    // an explicit limit asked for the list shape
    let (_, out, _) = fael(&d, &["find", "--kind", "note", "--limit", "2"]);
    assert!(!out.contains("body words"), "{out}");
}

/// A fat body would cost more than the `find <id>` it saves, so a short list
/// keeps its title; the ids pull is cut at the budget and names the ids left.
#[test]
fn fat_bodies_are_bounded_in_a_short_list_and_in_an_ids_pull() {
    let d = repo();
    let fat = "word ".repeat(900);
    let a = add(&d, "a.rs", "fat headline one", &fat);
    let b = add(&d, "b.rs", "fat headline two", &fat);
    let (_, out, _) = fael(&d, &["find", "--files", "src/a.rs"]);
    assert!(
        out.contains("fat headline one") && !out.contains("word word word"),
        "{out}"
    );
    // the first body always shows; the second is cut with the exact next call
    let (ok, out, err) = fael(&d, &["find", &a, &b]);
    assert!(ok, "{err}");
    assert!(
        out.matches("word word").count() >= 1 && !out.contains("fat headline two →"),
        "{out}"
    );
    assert!(
        out.contains("… +1 more — next: fael find ") && out.contains(&b[..12]),
        "{out}"
    );
}

/// `--json` is never cut, only priced: a script still gets every row, an agent
/// that reached for it sees the cost on stderr, and a named `--limit` is silent.
#[test]
fn json_over_the_budget_warns_on_stderr_and_stays_whole() {
    let d = repo();
    let fat = "word ".repeat(900);
    add(&d, "a.rs", "fat headline one", &fat);
    add(&d, "b.rs", "fat headline two", &fat);
    let (ok, out, err) = fael(&d, &["find", "--json", "--kind", "note"]);
    assert!(ok, "{err}");
    assert_eq!(out.lines().count(), 2, "every row, uncut");
    assert!(
        out.lines()
            .all(|l| serde_json::from_str::<serde_json::Value>(l).is_ok())
    );
    assert!(
        err.contains("2 rows") && err.contains("tokens of JSON") && err.contains("--limit"),
        "{err}"
    );
    let (_, out, err) = fael(&d, &["find", "--json", "--kind", "note", "--limit", "5"]);
    assert_eq!(out.lines().count(), 2);
    assert!(!err.contains("tokens of JSON"), "{err}");
    // a small result stays silent
    let e = repo();
    add(&e, "a.rs", "tiny", "tiny body");
    let (_, _, err) = fael(&e, &["find", "--json", "--kind", "note"]);
    assert!(!err.contains("tokens of JSON"), "{err}");
}

#[test]
fn an_empty_text_search_names_the_word_and_is_recorded() {
    let d = repo();
    add(
        &d,
        "a.rs",
        "ranking headline",
        "ranking stays deterministic",
    );
    let (ok, out, err) = fael(&d, &["find", "--text", "ranking vector"]);
    assert!(ok && out.is_empty(), "{out}");
    assert!(
        err.contains("\"ranking\" ×1") && err.contains("\"vector\" ×0"),
        "{err}"
    );
    // recorded for stats; a files-only miss is not a vocabulary miss
    let (_, _, _) = fael(&d, &["find", "--files", "src/none.rs"]);
    let (_, out, _) = fael(&d, &["stats", "--misses"]);
    assert!(
        out.contains("\"ranking vector\"") && !out.contains("none.rs"),
        "{out}"
    );
    assert_eq!(out.lines().count(), 1, "{out}");
    // the plain stats page grows one line only once something missed
    let (_, out, _) = fael(&d, &["stats"]);
    assert!(out.contains("find misses: ×1"), "{out}");
}

#[test]
fn mcp_find_ids_pulls_several_bodies_and_explains_an_empty_search() {
    let d = repo();
    let a = add(&d, "a.rs", "first headline", "first body words");
    let b = add(&d, "b.rs", "second headline", "second body words");
    let msgs = [
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"find","arguments":{{"ids":["{a}","{b}"]}}}}}}"#
        ),
        format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"find","arguments":{{"ids":["{a}","01DEFACED01"]}}}}}}"#
        ),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find","arguments":{"text":"first vector"}}}"#.to_string(),
    ];
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", root(&d).join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(&d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all((msgs.join("\n") + "\n").as_bytes())
        .unwrap();
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    let r: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let text = |i: usize| {
        r[i]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(r[0]["result"]["isError"], false, "{out}");
    assert!(text(0).contains("first body words") && text(0).contains("second body words"));
    assert_eq!(r[1]["result"]["isError"], true, "{out}");
    assert!(text(1).contains("first body words") && text(1).contains("rejected: no row with id"));
    assert!(
        text(2).contains("\"vector\" ×0") && text(2).contains("\"first\" ×1"),
        "{}",
        text(2)
    );
}
