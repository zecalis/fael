//! PLAN-fael-learn-loop chunk 2: observed outcomes. A said id typed into a
//! tool input or the closing reply is `cited` once per session; a `fael …`
//! command, a tool response and an id fael never said are not. `fael stats
//! --rows` joins the lines into per-row outcomes under `outcomes_v: 1`.

use super::{fael, fael_env, json, repo};
use serde_json::{Value, json};
use std::path::Path;

fn call(d: &Path, event: &str, session: &str, rest: &str) {
    let input = format!(r#"{{"cwd":{},"session":"{session}",{rest}}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", event], &input);
    assert!(ok, "{out}{err}");
}

fn usage(d: &Path) -> Vec<Value> {
    std::fs::read_to_string(d.join("state/usage.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn cited(d: &Path) -> Vec<Value> {
    usage(d)
        .into_iter()
        .filter(|l| l["event"] == "outcome")
        .map(|l| l["cited"].clone())
        .collect()
}

/// One decision on `src/a.rs`, pushed to session `s1` by an edit: its id.
fn said(d: &Path) -> String {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        d,
        &[
            "add",
            "decision",
            "keep the parser pure",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    call(d, "edit", "s1", r#""files":["src/a.rs"]"#);
    let edit = usage(d).into_iter().find(|l| l["event"] == "edit").unwrap();
    edit["ids"][0].as_str().unwrap().to_string()
}

fn row(d: &Path, id: &str) -> Value {
    let (ok, out, err) = fael(d, &["stats", "--json", "--rows"], "");
    assert!(ok, "{err}");
    let s: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(s["outcomes_v"], 1, "{out}");
    s["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("{id} not in {out}"))
        .clone()
}

#[test]
fn a_said_id_in_a_tool_input_is_cited_once() {
    let d = repo();
    let id = said(&d);
    let short = &id[..8];
    let edit = format!(r#""files":["src/b.rs"],"tool_input":{{"new_string":"// per {short}"}}"#);
    call(&d, "edit", "s1", &edit);
    assert_eq!(cited(&d), [json!([id])]);
    call(&d, "edit", "s1", &edit);
    assert_eq!(cited(&d).len(), 1, "once per session");
    let r = row(&d, &id);
    assert_eq!(r["outcomes"]["shown"], 1, "{r}");
    assert_eq!(r["outcomes"]["cited"], 1, "{r}");
}

#[test]
fn a_fael_command_a_response_and_an_unsaid_id_are_no_cite() {
    let d = repo();
    let id = said(&d);
    let short = &id[..8];
    // `fael find <id>` is a pull
    let cmd = format!(r#""tool":"Bash","tool_input":{{"command":"fael find {short}"}}"#);
    call(&d, "search", "s1", &cmd);
    // the id only in the tool's response
    let resp =
        format!(r#""tool":"Bash","tool_input":{{"command":"ls"}},"tool_response":"{short}""#);
    call(&d, "search", "s1", &resp);
    // another session was never told it
    let edit = format!(r#""files":["src/b.rs"],"tool_input":{{"new_string":"{short}"}}"#);
    call(&d, "edit", "s2", &edit);
    assert!(cited(&d).is_empty(), "{:?}", usage(&d));
    assert_eq!(row(&d, &id)["outcomes"]["cited"], 0);
}

#[test]
fn the_closing_reply_cites_a_said_id() {
    let d = repo();
    let id = said(&d);
    call(
        &d,
        "stop",
        "s1",
        &format!(r#""reply":"fixed, as {id} says""#),
    );
    assert_eq!(cited(&d), [json!([id])]);
}

#[test]
fn a_pull_is_agent_initiated_unless_fael_pointed_at_it() {
    let d = repo();
    let id = said(&d);
    let env = [("FAEL_SESSION", "s1")];
    let (ok, _, err) = fael_env(&d, &["find", &id], "", &env);
    assert!(ok, "{err}");
    let o = &row(&d, &id)["outcomes"];
    assert_eq!(
        o["pulled"],
        json!({"agent_initiated": 1, "fael_induced": 0})
    );
    assert_eq!(o["cited"], 0, "a pull is no cite: {o}");
}

#[test]
fn the_claude_adapter_cites_from_its_tool_input() {
    let d = repo();
    let id = said(&d);
    let short = &id[..8];
    // Claude Code's shape: `session_id`, an Edit's `tool_input` — the same
    // seen list the neutral edit above filled
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Edit","tool_input":{{"file_path":{},"new_string":"// per {short}"}}}}"#,
        json(&d),
        json(&d.join("src/b.rs"))
    );
    let (ok, out, err) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok, "{out}{err}");
    assert_eq!(cited(&d), [json!([id])]);
    // a Bash call reaches the same door through `search`
    let bash = format!(
        r#"{{"cwd":{},"session_id":"s2","tool_name":"Bash","tool_input":{{"command":"git commit -m 'fix {short}'"}},"tool_response":{{}}}}"#,
        json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "search", "--client", "claude"], &bash);
    assert!(ok, "{out}{err}");
    assert_eq!(cited(&d).len(), 1, "s2 was never told the row");
}

#[test]
fn a_pull_after_a_cap_cut_is_the_agents_own() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    for i in 0..7 {
        let t = format!("decision {i}");
        let (ok, _, err) = fael(
            &d,
            &["add", "decision", &t, "--files", "src/a.rs", "--force"],
            "",
        );
        assert!(ok, "{err}");
    }
    call(&d, "edit", "s1", r#""files":["src/a.rs"]"#);
    let pushed = usage(&d)
        .into_iter()
        .find(|l| l["event"] == "edit")
        .unwrap();
    let kinds: Vec<&str> = pushed["said"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["kind"].as_str().unwrap())
        .collect();
    // no count line names the call to the cut rows: nothing fael said induces a pull
    assert!(!kinds.contains(&"count"), "{pushed}");
    let cut: Vec<&str> = pushed["cut"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert!(cut.len() >= 2, "{pushed}");
    let env = [("FAEL_SESSION", "s1")];
    // the agent asks for one cut row by id
    let (ok, _, err) = fael_env(&d, &["find", cut[0]], "", &env);
    assert!(ok, "{err}");
    // and the rest by the file
    let (ok, _, err) = fael_env(&d, &["find", "--files", "src/a.rs"], "", &env);
    assert!(ok, "{err}");
    let o = &row(&d, cut[0])["outcomes"];
    assert_eq!(o["retrieved_after_cut"], 1, "{o}");
    assert_eq!(o["missed_push"], 0, "a cap cut is no policy cut: {o}");
    let o = &row(&d, cut[1])["outcomes"];
    assert_eq!(o["retrieved_after_cut"], 1, "{o}");
    // a row said, then shown again by the agent's own file pull
    let said_id = pushed["ids"][0].as_str().unwrap();
    let o = &row(&d, said_id)["outcomes"];
    assert_eq!(
        o["pulled"],
        json!({"agent_initiated": 1, "fael_induced": 0})
    );
}

#[test]
fn a_sub_agent_cites_from_its_own_reply_and_seen_list() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "keep the parser pure",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    // the sub-agent's own context was told the row
    call(&d, "edit", "s1", r#""agent":"a1","files":["src/a.rs"]"#);
    let pushed = usage(&d).into_iter().find(|l| l["event"] == "edit");
    let id = pushed.expect("the sub-agent was told a row")["ids"][0]
        .as_str()
        .unwrap()
        .to_string();
    // the session's own thread never was: its stop cites nothing
    call(&d, "stop", "s1", &format!(r#""reply":"as {id} says""#));
    assert!(cited(&d).is_empty(), "{:?}", usage(&d));
    call(
        &d,
        "stop",
        "s1",
        &format!(r#""agent":"a1","reply":"as {id} says""#),
    );
    assert_eq!(cited(&d), [json!([id])]);
}

/// 450 listed PRs over two pages hold all 450 in the join.
#[test]
fn a_paged_pr_list_holds_all_450() {
    let d = repo();
    // two pages: 200 + 250, newest first
    let mut prs = vec![];
    for n in 1..=450 {
        let day = 1 + (n % 28);
        prs.push(format!(
            r#"{{"number":{n},"headRefName":"feat/{n}","state":"MERGED","createdAt":"2026-10-{day:02}T00:00:00Z","mergedAt":"2026-10-{day:02}T01:00:00Z"}}"#
        ));
    }
    let page1 = format!("[{}]", prs[..200].join(","));
    let page2 = format!("[{}]", prs[200..].join(","));
    // the join reads one list: pages concatenated hold all 450
    let combined: Vec<serde_json::Value> = [page1, page2]
        .iter()
        .flat_map(|p| serde_json::from_str::<Vec<serde_json::Value>>(p).unwrap())
        .collect();
    assert_eq!(combined.len(), 450);
    let all = serde_json::to_string(&combined).unwrap();
    let (ok, out, err) = fael_env(
        &d,
        &["stats", "--json"],
        "",
        &[("FAEL_GH_JSON", all.as_str()), ("FAEL_GH_COUNT", "450")],
    );
    assert!(ok, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    let listed = v["outcome"]["prs_with_row"].as_u64().unwrap_or(0)
        + v["outcome"]["prs_without_row"].as_u64().unwrap_or(0);
    assert_eq!(listed, 450, "{out}");
}

/// When the list is shorter than the search total the text names the gap.
#[test]
fn a_short_list_prints_listed_n_of_m() {
    let d = repo();
    let mut prs = vec![];
    for n in 1..=200 {
        prs.push(format!(
            r#"{{"number":{n},"headRefName":"feat/{n}","state":"MERGED","createdAt":"2026-10-10T00:00:00Z","mergedAt":"2026-10-10T01:00:00Z"}}"#
        ));
    }
    let all = format!("[{}]", prs.join(","));
    let (ok, out, err) = fael_env(
        &d,
        &["stats", "--since", "2026-10-01"],
        "",
        &[("FAEL_GH_JSON", all.as_str()), ("FAEL_GH_COUNT", "450")],
    );
    assert!(ok, "{err}");
    assert!(out.contains("listed 200 of 450"), "{out}");
}

/// `fael stats --session` ties a session to the listed PR whose head is the
/// branch on its edit lines: two sessions on one branch both show it, a
/// branch with no listed PR says so, and the split tables count merged ones.
#[test]
fn a_session_shows_the_pr_its_branch_heads() {
    let d = repo();
    let line = |ts: &str, s: &str, branch: &str| {
        format!(
            r#"{{"ts":"{ts}","repo":{},"client":"claude","session":"{s}","event":"edit","bytes":0,"est_tokens":0,"ids":[],"branch":"{branch}","files":["a.rs"]}}"#,
            json(&d.canonicalize().unwrap())
        )
    };
    let body = [
        line("2026-10-02T00:00:00.000Z", "sess-a1", "feat/x"),
        line("2026-10-03T00:00:00.000Z", "sess-a2", "feat/x"),
        line("2026-10-04T00:00:00.000Z", "sess-c3", "feat/none"),
    ]
    .join("\n");
    std::fs::create_dir_all(d.join("state")).unwrap();
    std::fs::write(d.join("state/usage.jsonl"), body + "\n").unwrap();
    let prs = r#"[{"number":41,"headRefName":"feat/x","state":"MERGED","createdAt":"2026-10-02T00:00:00Z","mergedAt":"2026-10-05T01:00:00Z"}]"#;
    let env = [("FAEL_GH_JSON", prs)];
    for (s, want) in [
        ("sess-a1", "PR #41 merged 2026-10-05"),
        ("sess-a2", "PR #41 merged 2026-10-05"),
        ("sess-c3", "no PR listed"),
    ] {
        let (ok, out, err) = fael_env(&d, &["stats", "--session", s], "", &env);
        assert!(ok, "{err}");
        assert!(out.contains(want), "{s}: {want} missing: {out}");
    }
    let (ok, out, err) = fael_env(&d, &["stats", "sessions"], "", &env);
    assert!(ok, "{err}");
    assert!(out.contains("with a merged PR 2"), "{out}");
}
