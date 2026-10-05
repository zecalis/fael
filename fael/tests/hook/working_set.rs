//! PLAN-fael-learn-loop chunk 3: `feat.touch` counts a row's files already in
//! the session's working set (the files its earlier pushes were on), and
//! `would_drop` names the said rows the shadow policy `touch@1` would have
//! cut — recorded only, the agent still saw every one.

use super::{fael, fael_env, json, repo, state};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn add(d: &Path, kind: &str, text: &str, files: &str) {
    for f in files.split(',') {
        std::fs::write(d.join(f), "// x\n").unwrap();
    }
    let (ok, _, err) = fael(d, &["add", kind, text, "--files", files], "");
    assert!(ok, "{err}");
}

/// The id of the row whose text is `text` (`find --json` prints row lines).
pub(super) fn id_of(d: &Path, text: &str) -> String {
    let (_, out, _) = fael(d, &["find", text, "--json"], "");
    out.lines()
        .map(|l| serde_json::from_str::<Value>(l).expect(l))
        .find(|r| r["text"] == text)
        .expect(&out)["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The last usage line of `event`.
fn line(d: &Path, event: &str) -> Value {
    let usage = std::fs::read_to_string(state(d).join("usage.jsonl")).unwrap();
    usage
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).expect(l))
        .rfind(|l| l["event"] == event)
        .expect(&usage)
}

/// A read of `file` by `session` — by the sub-agent `agent`, when not empty.
fn read(d: &Path, session: &str, agent: &str, file: &str) {
    let agent = if agent.is_empty() {
        String::new()
    } else {
        format!(r#""agent":"{agent}","#)
    };
    let input = format!(
        r#"{{"cwd":{},"session":"{session}",{agent}"files":["{file}"]}}"#,
        json(d)
    );
    let (ok, out, err) = fael(d, &["hook", "read"], &input);
    assert!(ok, "{out}{err}");
}

/// A Grep whose hit list names `files`, through the Claude adapter.
pub(super) fn grep(d: &Path, session: &str, files: &[&str]) {
    let p = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_name":"Grep","tool_input":{{"pattern":"x"}},"tool_response":{{"filenames":{}}}}}"#,
        json(d),
        json!(files)
    );
    let (ok, _, err) = fael(d, &["hook", "search", "--client", "claude"], &p);
    assert!(ok, "{err}");
}

fn touch(l: &Value, id: &str) -> Value {
    l["feat"][id]["touch"].clone()
}

#[test]
fn touch_counts_the_working_set_and_would_drop_names_the_untouched() {
    let d = repo();
    add(&d, "issue", "login loops", "src/a.rs");
    read(&d, "s1", "", "src/a.rs");
    let first = line(&d, "read");
    // nothing touched before the first push; the issue is never dropped
    let issue = first["ids"][0].as_str().unwrap().to_string();
    assert_eq!(touch(&first, &issue), 0, "{first}");
    assert!(first.get("would_drop").is_none(), "{first}");

    // filed after a.rs was touched: one row spans it, one is b.rs alone
    add(&d, "decision", "spans both", "src/a.rs,src/b.rs");
    add(&d, "decision", "only b", "src/b.rs");
    read(&d, "s1", "", "src/b.rs");
    let l = line(&d, "read");
    let (span, only) = (id_of(&d, "spans both"), id_of(&d, "only b"));
    assert_eq!(touch(&l, &span), 1, "{l}");
    assert_eq!(touch(&l, &only), 0, "{l}");
    assert_eq!(
        l["would_drop"],
        json!({"policy": "touch@1", "ids": [only]}),
        "{l}"
    );
    // shadow only: both rows were said
    let said = l["ids"].as_array().unwrap();
    assert!(
        said.contains(&json!(span)) && said.contains(&json!(only)),
        "{l}"
    );
}

#[test]
fn no_session_means_no_working_set() {
    let d = repo();
    add(&d, "decision", "d", "src/a.rs");
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, err) = fael(&d, &["hook", "read"], &input);
    assert!(ok, "{out}{err}");
    let l = line(&d, "read");
    let id = l["ids"][0].as_str().unwrap();
    assert!(l["feat"][id].get("touch").is_none(), "{l}");
    assert!(l.get("would_drop").is_none(), "{l}");
}

/// Only a row the push said can be dropped from what the agent saw: a row the
/// row cap or the token budget cut keeps its `touch` but never rides `would_drop`.
#[test]
fn would_drop_holds_said_rows_only() {
    for cfg in ["push_rows = 1", "push_tokens = 40"] {
        let d = repo();
        for t in ["one", "two", "three", "four"] {
            add(&d, "decision", t, "src/a.rs");
        }
        std::fs::write(d.join(".fael/config.toml"), format!("[budget]\n{cfg}\n")).unwrap();
        read(&d, "s1", "", "src/a.rs");
        let l = line(&d, "read");
        let said = l["ids"].as_array().unwrap();
        let cut = l["cut"].as_array().unwrap();
        assert!(!said.is_empty() && !cut.is_empty(), "{cfg}: {l}");
        // every row untouched: the dropped ones are exactly the said ones
        assert_eq!(l["would_drop"]["ids"], json!(said), "{cfg}: {l}");
        for c in cut {
            assert_eq!(touch(&l, c["id"].as_str().unwrap()), 0, "{cfg}: {l}");
        }
    }
}

/// A hit list over several files pushes nothing, so it touches nothing; one
/// that names a single file is a touch like a named one.
#[test]
fn only_a_single_file_hit_list_is_a_touch() {
    let d = repo();
    for f in ["src/a.rs", "src/b.rs", "src/c.rs"] {
        std::fs::write(d.join(f), "// x\n").unwrap();
    }
    grep(&d, "t1", &["src/a.rs", "src/c.rs"]); // two files: no push, no touch
    add(&d, "decision", "spans c and b", "src/c.rs,src/b.rs");
    grep(&d, "t1", &["src/b.rs"]);
    let l = line(&d, "search");
    assert_eq!(l["trigger"], "hitlist", "{l}");
    assert_eq!(touch(&l, &id_of(&d, "spans c and b")), 0, "{l}");

    grep(&d, "t2", &["src/a.rs"]); // one file: a touch
    add(&d, "decision", "spans a and b", "src/a.rs,src/b.rs");
    grep(&d, "t2", &["src/b.rs"]);
    let l = line(&d, "search");
    assert_eq!(touch(&l, &id_of(&d, "spans a and b")), 1, "{l}");
}

/// A sub-agent starts with an empty context: its working set is its own.
#[test]
fn a_sub_agent_keeps_its_own_working_set() {
    let d = repo();
    read(&d, "s1", "", "src/a.rs");
    add(&d, "decision", "spans both", "src/a.rs,src/b.rs");
    let id = id_of(&d, "spans both");
    read(&d, "s1", "a1", "src/b.rs");
    let l = line(&d, "read");
    assert_eq!(touch(&l, &id), 0, "a1 never touched a.rs: {l}");
    read(&d, "s1", "", "src/b.rs");
    let l = line(&d, "read");
    assert_eq!(touch(&l, &id), 1, "the session's own thread did: {l}");
}

/// Parallel reads queue behind the seen lock: every file lands in the working
/// set once, none lost to an interleaved append.
#[test]
fn parallel_reads_keep_the_working_set_whole() {
    let d = repo();
    let files: Vec<String> = (0..8).map(|i| format!("src/f{i}.rs")).collect();
    for f in &files {
        std::fs::write(d.join(f), "// x\n").unwrap();
    }
    let handles: Vec<_> = files
        .iter()
        .map(|f| {
            let (d, f) = (d.clone(), f.clone());
            std::thread::spawn(move || read(&d, "s1", "", &f))
        })
        .collect();
    handles.into_iter().for_each(|h| h.join().unwrap());
    let dir = state(&d).join("sessions");
    let set: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "touched"))
        .flat_map(|e| {
            std::fs::read_to_string(e.path())
                .unwrap()
                .lines()
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect();
    let mut want = files.clone();
    want.sort();
    let mut got = set;
    got.sort();
    assert_eq!(got, want);
}

fn rows(d: &Path) -> Value {
    let (ok, out, err) = fael(d, &["stats", "--json", "--rows"], "");
    assert!(ok, "{err}");
    serde_json::from_str(&out).unwrap()
}

/// The writer feeds the join: a row `touch@1` would have dropped, pulled by the
/// agent itself and then cited, is a `missed_push` — the SPEC §7 example.
#[test]
fn a_would_drop_row_the_agent_pulls_and_cites_is_a_missed_push() {
    let d = repo();
    add(&d, "decision", "keep the parser pure", "src/a.rs");
    let id = id_of(&d, "keep the parser pure");
    read(&d, "s1", "", "src/a.rs");
    assert_eq!(line(&d, "read")["would_drop"]["ids"], json!([id]));
    let (ok, _, err) = fael_env(&d, &["find", &id], "", &[("FAEL_SESSION", "s1")]);
    assert!(ok, "{err}");
    let edit = format!(
        r#"{{"cwd":{},"session":"s1","files":["src/b.rs"],"tool_input":{{"new_string":"// per {}"}}}}"#,
        json(&d),
        &id[..8]
    );
    let (ok, out, err) = fael(&d, &["hook", "edit"], &edit);
    assert!(ok, "{out}{err}");
    let s = rows(&d);
    let r = s["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    let o = &r["outcomes"];
    assert_eq!(o["cut"], json!({"would_drop": 1}), "{o}");
    assert_eq!(o["retrieved_after_cut"], 1, "{o}");
    assert_eq!(o["missed_push"], 1, "{o}");
}
