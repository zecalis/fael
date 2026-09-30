//! The files a Grep or a Bash read touched — the push for agents that read
//! through the shell instead of `Read` (fresh agents made 0 `Read` calls in 15
//! sessions). Nothing is guessed: a path counts only when it names a file that
//! exists, and only a reader command (`cat`, `sed`, `grep`, `git show` …)
//! makes its arguments a touch.

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

/// Files touched by one Grep/Bash tool call. `input` is `tool_input`,
/// `response` is `tool_response` (a string, or an object with `stdout`,
/// `content` or `filenames`).
pub(crate) fn touched(tool: &str, input: &Value, response: &Value, cwd: &Path) -> Vec<String> {
    let is_file = |p: &str| !p.is_empty() && cwd.join(p).is_file();
    let mut out: Vec<String> = vec![];
    let mut add = |p: &str| {
        if out.len() < MAX_FILES && is_file(p) && !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    };
    let mut listing = tool == "Grep";
    if tool == "Grep" {
        if let Some(p) = input["path"].as_str() {
            add(p);
        }
    } else if tool == "Bash" {
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
            r => {
                for k in ["filenames", "stdout", "content"] {
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
