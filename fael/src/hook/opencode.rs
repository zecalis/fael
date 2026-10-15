//! `opencode` adapter: the single OpenCode normalization point between
//! `fael/skill/opencode.js` (thin mapping) and core.
//! The plugin speaks the neutral protocol, so every OpenCode event arrives
//! as a neutral `Event` with `client == "opencode"`; `normalize` shapes it —
//! V1-vs-V2 payload differences, tool-name routing — and the neutral arm
//! runs the same core on the result. Anything the adapter leaves alone
//! replies byte-identical: this module moves the rules home, it changes no
//! reply. The differences the wire already resolved stay in the JS
//! (session-start time as RFC 3339, stop text/reply strings); the adapter
//! owns their contract and proves both shapes flow (fixtures), so the later
//! chunks (prompt, notice, stop, shell-session) can resolve server-side here.

use super::protocol::Event;
use serde_json::Value;

/// Tools that edit, as `opencode.js` `EDIT` — V1 (`apply_patch`) and V2
/// (`patch`) spellings both.
pub(crate) const EDIT: [&str; 4] = ["edit", "write", "patch", "apply_patch"];
/// Tools that read like a shell read, as `opencode.js` `SEARCH`.
pub(crate) const SEARCH: [&str; 4] = ["grep", "glob", "shell", "bash"];
/// The shell tools only, as `opencode.js` `SHELL` — the shells-map the
/// `oc-shell` chunk resolves server-side reads from; kept here so the three
/// sets stay in step. Unused until then.
#[allow(dead_code)]
pub(crate) const SHELL: [&str; 2] = ["shell", "bash"];

/// Stop text/reply extraction, mirroring `opencode.js` `stop_texts` (kept in
/// step, never imported: the JS runs in the plugin, this is its executable
/// spec). V1 assistant messages carry `parts[]`, V2 `content[]`; both keep
/// `type == "text"` parts only, `text` is every assistant message joined with
/// `\n`, `reply` the last assistant message only. Anything missing, empty or
/// non-string is `""` — idle never answers with a prompt, and stop fails
/// open on shapes it does not know. Spec mirror, exercised by the tests:
/// the plugin's JS runs the live copy.
#[allow(dead_code)]
fn stop_texts(messages: &Value, v1: bool) -> (String, String) {
    let is_assistant = |m: &Value| {
        if v1 {
            m.get("info")
                .and_then(|i| i.get("role"))
                .and_then(Value::as_str)
                == Some("assistant")
        } else {
            m.get("type").and_then(Value::as_str) == Some("assistant")
        }
    };
    let parts = |m: &Value| {
        m.get(if v1 { "parts" } else { "content" })
            .and_then(Value::as_array)
            .map(|ps| {
                ps.iter()
                    .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
                    .map(|p| {
                        p.get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    })
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default()
    };
    let asked: Vec<&Value> = messages
        .as_array()
        .map(|ms| ms.iter().filter(|m| is_assistant(m)).collect())
        .unwrap_or_default();
    // flatMap, as the JS: one empty assistant adds no segment
    let text = asked
        .iter()
        .flat_map(|m| parts(m))
        .collect::<Vec<_>>()
        .join("\n");
    let reply = asked
        .last()
        .map(|m| parts(m).join("\n"))
        .unwrap_or_default();
    (text, reply)
}

/// V1 idle payloads (`client.session.messages` → `data`), as `v1_stop` reads
/// them: the assistant messages' `parts[].text`.
#[allow(dead_code)]
pub(crate) fn v1_stop(data: &Value) -> (String, String) {
    stop_texts(data, true)
}

/// V2 idle payloads (`ctx.session.context`), as `v2_stop` reads them: the
/// assistant messages' `content[].text`.
#[allow(dead_code)]
pub(crate) fn v2_stop(messages: &Value) -> (String, String) {
    stop_texts(messages, false)
}

/// The event `opencode.js` `tool.execute.after` would spawn for `tool` —
/// `read`, `edit`, `search`, or none (it returns early). Names match
/// case-insensitively, the server convention (`search.rs`).
pub(crate) fn classify(tool: &str) -> Option<&'static str> {
    let t = tool.to_ascii_lowercase();
    if t == "read" {
        Some("read")
    } else if EDIT.contains(&t.as_str()) {
        Some("edit")
    } else if SEARCH.contains(&t.as_str()) {
        Some("search")
    } else {
        None
    }
}

/// Patch headers, the same four `claude.rs` `patch_files` and `opencode.js`
/// `PATCH_HEADERS` read (kept in step, never imported: `claude.rs` is
/// read-only for this plan).
fn patch_files(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|l| {
            [
                "*** Add File: ",
                "*** Update File: ",
                "*** Delete File: ",
                "*** Move to: ",
            ]
            .iter()
            .find_map(|h| l.strip_prefix(h))
        })
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// The files a tool call touched: V2 names it `path`, V1 `filePath`;
/// `patch`/`apply_patch` carry no path, so read the patch headers.
/// Mirrors `opencode.js` `paths`.
pub(crate) fn paths(tool: &str, input: &Value) -> Vec<String> {
    let t = tool.to_ascii_lowercase();
    if t == "patch" || t == "apply_patch" {
        return match input.get("patchText").and_then(Value::as_str) {
            Some(p) => patch_files(p),
            None => vec![],
        };
    }
    match input
        .get("path")
        .or_else(|| input.get("filePath"))
        .and_then(Value::as_str)
    {
        Some(p) if !p.is_empty() => vec![p.to_string()],
        _ => vec![],
    }
}

/// Shape an OpenCode event for the neutral arms: stamp `client`, and derive
/// the touched files when a read/edit names its tool call but no files —
/// the plugin always sends files today, so this fires only on inputs it
/// never produces, and every such reply stays what neutral says.
///
/// `prompt` passes straight through: the V2 `prompt` admission hook hands
/// the JS the user's text (`event.prompt.text`), the JS forwards it as
/// `text`, and `prompt.rs` matches it — the adapter never sees the raw
/// admission shape, so there is nothing to extract here. (V1's
/// `chat.message` is deliberately unwired: its payload shape is unverified
/// against a V1 runtime, this machine runs V2, and V1 never fired prompt —
/// no regression. Revisit only with a V1 host in hand.)
pub(crate) fn normalize(event: &str, e: &Event) -> Event {
    let mut out = e.clone();
    out.client = Some("opencode".into());
    if out.files.is_empty()
        && let Some(tool) = e.tool.as_deref()
        && let Some(kind) = classify(tool)
        && kind == event
        && matches!(event, "read" | "edit")
    {
        out.files = paths(tool, &e.tool_input);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event() -> Event {
        Event::default()
    }

    #[test]
    fn classify_mirrors_the_js_routing() {
        for t in ["read", "Read", "READ"] {
            assert_eq!(classify(t), Some("read"), "{t}");
        }
        for t in ["edit", "write", "patch", "apply_patch", "Apply_Patch"] {
            assert_eq!(classify(t), Some("edit"), "{t}");
        }
        for t in ["grep", "glob", "shell", "bash", "Grep", "BASH"] {
            assert_eq!(classify(t), Some("search"), "{t}");
        }
        for t in ["", "frobnicate", "read-like"] {
            assert_eq!(classify(t), None, "{t}");
        }
    }

    #[test]
    fn paths_reads_v2_path_then_v1_filepath() {
        assert_eq!(
            paths(
                "write",
                &json!({"path": "src/a.rs", "filePath": "src/b.rs"})
            ),
            ["src/a.rs"]
        );
        assert_eq!(
            paths("read", &json!({"filePath": "src/a.rs"})),
            ["src/a.rs"]
        );
        assert_eq!(paths("read", &json!({"path": ""})), Vec::<String>::new());
        assert_eq!(paths("read", &json!({})), Vec::<String>::new());
    }

    #[test]
    fn paths_reads_patch_headers_both_spellings() {
        let patch = "*** Begin Patch\n*** Add File: src/a.rs\n*** Update File: src/b.rs\n*** Delete File: src/c.rs\n*** Move to: src/d.rs\n*** End Patch";
        let want = ["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs"];
        assert_eq!(paths("patch", &json!({"patchText": patch})), want);
        assert_eq!(paths("apply_patch", &json!({"patchText": patch})), want);
        assert_eq!(paths("patch", &json!({})), Vec::<String>::new());
    }

    #[test]
    fn shell_is_the_shell_search_subset() {
        for t in SHELL {
            assert!(SEARCH.contains(&t), "{t}");
            assert_eq!(classify(t), Some("search"), "{t}");
        }
    }

    #[test]
    fn normalize_stamps_client_and_derives_on_agreement() {
        let mut e = event();
        e.tool = Some("write".into());
        e.tool_input = json!({"path": "src/a.rs"});
        let n = normalize("edit", &e);
        assert_eq!(n.client.as_deref(), Some("opencode"));
        assert_eq!(n.files, ["src/a.rs"]);
        // a search event keeps its empty files: push_call resolves them
        let n = normalize("search", &e);
        assert!(n.files.is_empty());
        // event and tool disagree: never second-guess the routing
        let n = normalize("read", &e);
        assert!(n.files.is_empty());
        // files as sent win over anything derived
        let mut e = event();
        e.files = vec!["src/sent.rs".into()];
        e.tool = Some("write".into());
        e.tool_input = json!({"path": "src/other.rs"});
        let n = normalize("edit", &e);
        assert_eq!(n.files, ["src/sent.rs"]);
    }

    #[test]
    fn normalize_passes_prompt_text_through() {
        // the V2 prompt admission hook's text, as the JS forwards it: the
        // adapter stamps the client and touches nothing else, so `prompt.rs`
        // matches exactly what the user typed
        let mut e = event();
        e.session = Some("2026-01-02T00:00:00Z".into());
        e.text = Some("is there credit ledger code yet?".into());
        let n = normalize("prompt", &e);
        assert_eq!(n.client.as_deref(), Some("opencode"));
        assert_eq!(n.text.as_deref(), Some("is there credit ledger code yet?"));
        assert_eq!(n.session.as_deref(), Some("2026-01-02T00:00:00Z"));
        assert!(n.files.is_empty());
    }

    /// Two turns: `text` carries both, `reply` only the last — the split
    /// `stop.rs` needs (whole turn for the bug rule, last message for cited).
    #[test]
    fn stop_splits_text_and_reply() {
        let v1 = json!([
            {"info": {"role": "assistant"}, "parts": [{"type": "text", "text": "first turn"}]},
            {"info": {"role": "user"}, "parts": [{"type": "text", "text": "go on"}]},
            {"info": {"role": "assistant"}, "parts": [{"type": "text", "text": "fael issue: login loops"}]},
        ]);
        assert_eq!(
            v1_stop(&v1),
            (
                "first turn\nfael issue: login loops".into(),
                "fael issue: login loops".into()
            )
        );
        let v2 = json!([
            {"type": "assistant", "content": [{"type": "text", "text": "first turn"}]},
            {"type": "user", "content": [{"type": "text", "text": "go on"}]},
            {"type": "assistant", "content": [{"type": "text", "text": "fael issue: login loops"}]},
        ]);
        assert_eq!(
            v2_stop(&v2),
            (
                "first turn\nfael issue: login loops".into(),
                "fael issue: login loops".into()
            )
        );
    }

    #[test]
    fn stop_ignores_non_text_parts_and_empty_shapes() {
        // tool and reasoning parts never reach stop; non-assistants neither
        let v2 = json!([
            {"type": "assistant", "content": [
                {"type": "reasoning", "text": "hmm"},
                {"type": "text", "text": "wired the adapter"},
                {"type": "tool", "text": "read"},
            ]},
        ]);
        assert_eq!(
            v2_stop(&v2),
            ("wired the adapter".into(), "wired the adapter".into())
        );
        // empty, missing and misshapen payloads idle silently: ("", "")
        for (name, msgs) in [
            ("empty array", json!([])),
            (
                "no assistants",
                json!([{"type": "user", "content": [{"type": "text", "text": "hi"}]}]),
            ),
            ("missing content", json!([{"type": "assistant"}])),
            (
                "missing text",
                json!([{"type": "assistant", "content": [{"type": "text"}]}]),
            ),
            (
                "non-string text",
                json!([{"type": "assistant", "content": [{"type": "text", "text": 7}]}]),
            ),
            ("not an array", json!({"type": "assistant"})),
            ("null", json!(null)),
        ] {
            assert_eq!(v2_stop(&msgs), (String::new(), String::new()), "{name}");
        }
        for (name, data) in [
            ("empty array", json!([])),
            ("missing parts", json!([{"info": {"role": "assistant"}}])),
            ("null", json!(null)),
        ] {
            assert_eq!(v1_stop(&data), (String::new(), String::new()), "{name}");
        }
    }
}
