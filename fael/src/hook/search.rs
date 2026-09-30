//! The files a search or a shell read touched — the push for agents that read
//! through search tools or the shell instead of `Read` (fresh agents made 0
//! `Read` calls in 15 sessions). Nothing is guessed: a path counts only when
//! it names a file that exists, and only a reader command (`cat`, `sed`,
//! `grep`, `git show` …) makes its arguments a touch. Tool names match
//! case-insensitively: Claude sends `Grep`/`Bash`/`Glob`, OpenCode `grep`/
//! `bash`/`glob`, and Codex shell calls arrive as `Bash` (unified exec included).

use serde_json::Value;
use std::path::Path;

/// A hit list can name hundreds of files — the push looks at the first few.
const MAX_FILES: usize = 8;
/// Output lines scanned for hit-list paths (each costs one `stat`).
const MAX_LINES: usize = 200;
const READERS: [&str; 12] = [
    "cat", "head", "tail", "sed", "awk", "nl", "less", "more", "bat", "grep", "egrep", "rg",
];
const GIT_READERS: [&str; 5] = ["show", "diff", "log", "blame", "grep"];
/// Shell tool names beyond `Bash` (Codex unified exec and OpenCode aliases).
const SHELLS: [&str; 5] = ["bash", "shell", "exec", "exec_command", "shell_command"];
/// Hit-list fields of a search response (Claude `filenames`, shell `stdout`,
/// Codex `output`).
const HIT_KEYS: [&str; 5] = ["filenames", "stdout", "content", "output", "result"];

/// Files touched by one search/shell tool call. `input` is `tool_input`,
/// `response` is `tool_response` (a string, or an object with `stdout`,
/// `content`, `output`, `result` or `filenames`).
pub(crate) fn touched(tool: &str, input: &Value, response: &Value, cwd: &Path) -> Vec<String> {
    let is_file = |p: &str| !p.is_empty() && cwd.join(p).is_file();
    let mut out: Vec<String> = vec![];
    let mut add = |p: &str| {
        if out.len() < MAX_FILES && is_file(p) && !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    };
    let tool = tool.to_ascii_lowercase();
    let mut listing = tool == "grep" || tool == "glob";
    if tool == "grep" || tool == "glob" {
        // Grep (`path`) and Glob (`path`, OpenCode also `filePath`): a pattern
        // alone never names a file, and `add` drops it when it is not one.
        if let Some(p) = input["path"]
            .as_str()
            .or_else(|| input["filePath"].as_str())
        {
            add(p);
        }
    } else if SHELLS.contains(&tool.as_str()) {
        for seg in input["command"]
            .as_str()
            .unwrap_or("")
            .split(['|', ';', '&', '\n'])
        {
            let words: Vec<&str> = seg.split_whitespace().collect();
            let (git, rest) = match words.split_first() {
                Some((&"git", r)) => (true, r),
                Some(_) => (false, &words[..]),
                None => continue,
            };
            let reader = match rest.split_first() {
                Some((c, _)) if !git => c.rsplit('/').next().is_some_and(|c| READERS.contains(&c)),
                Some((c, _)) => GIT_READERS.contains(c),
                None => false,
            };
            if !reader {
                continue;
            }
            listing |= rest
                .first()
                .is_some_and(|c| c.ends_with("grep") || c.ends_with("rg"));
            for w in rest.iter().skip(1).filter(|w| !w.starts_with('-')) {
                let w = w.trim_matches(['\'', '"']);
                // `git show HEAD:src/a.rs`
                add(w.rsplit_once(':').filter(|_| git).map_or(w, |(_, p)| p));
            }
        }
    }
    if listing {
        let mut lines: Vec<&str> = vec![];
        match response {
            Value::String(s) => lines.extend(s.lines()),
            // a bare list (`Glob` on some clients) is already file paths
            Value::Array(a) => lines.extend(a.iter().filter_map(Value::as_str)),
            r => {
                for k in HIT_KEYS {
                    match &r[k] {
                        Value::String(s) => lines.extend(s.lines()),
                        Value::Array(a) => lines.extend(a.iter().filter_map(Value::as_str)),
                        _ => {}
                    }
                }
            }
        }
        for l in lines.into_iter().take(MAX_LINES) {
            // `path:line:text`, or a bare path (`grep -l`, `rg -l`, files_with_matches)
            add(l.split_once(':').map_or(l, |(p, _)| p).trim());
        }
    }
    out
}
