//! The `find`/`kickoff` call rebuilt for the cut line.

use super::quoted;
use crate::Args;

// Moved out of main.rs (file-size ratchet) — no logic of its own. A trait because `Args`
// lives in fael-core.
pub(crate) trait PageBase {
    fn page_base(&self, cmd: &str, positional: Option<&str>, limit: Option<usize>) -> String;
}

impl PageBase for Args {
    /// Rebuild this `find`/`kickoff` call for the cut line: the same filters,
    /// so the agent reruns it with the new `--offset` the renderer appends.
    /// Kickoff takes no filter flags, only `--full` and `--limit`.
    fn page_base(&self, cmd: &str, positional: Option<&str>, limit: Option<usize>) -> String {
        let mut s = format!("fael {cmd}");
        if let Some(p) = positional.filter(|p| !p.is_empty()) {
            s.push_str(&format!(" {}", quoted(p)));
        }
        if cmd == "find" {
            let fs = self.files();
            if !fs.is_empty() {
                s.push_str(&format!(" --files {}", quoted(&fs.join(","))));
            }
            // `--text` replays as the flag — the positional alone would id-match
            if let Some(t) = self.one("text") {
                s.push_str(&format!(" --text {}", quoted(&t)));
            }
            for f in ["key", "kind", "since", "by", "to", "recall"] {
                if let Some(v) = self.one(f) {
                    s.push_str(&format!(" --{f} {}", quoted(&v)));
                }
            }
            // bare `--revisit` replays bare; a value replays with it
            if self.has("revisit") {
                match self.one("revisit") {
                    Some(v) => s.push_str(&format!(" --revisit {}", quoted(&v))),
                    None => s.push_str(" --revisit"),
                }
            }
            if self.has("all") {
                s.push_str(" --all");
            }
        }
        if self.has("full") {
            s.push_str(" --full");
        }
        if let Some(n) = limit {
            s.push_str(&format!(" --limit {n}"));
        }
        s
    }
}
