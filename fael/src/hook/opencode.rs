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
}
