//! A brief naming a symbol no file under its scope holds (issue 01M73C23D):
//! a warning only, never a refusal. Backticked tokens that are one identifier
//! or an `a::b` path are searched as fixed strings in the files under the
//! scope; for `a::b` the last segment is searched. Same skip rules as
//! `brief_paths`: "leave alone" lines, `Check:`/`Run:`/`Verify:` clauses and
//! `chunk_scope_skip` (`do not touch`, `run`, `read`) never warn. A line with
//! `new`/`add` before the token never warns either: the chunk creates it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Warnings for backticked symbols in `title`/`brief` with no match in the
/// files under `scope`. Ordered, deduped, capped at 10 plus a count line.
pub(crate) fn warnings(root: &Path, scope: &[String], title: &str, brief: &str) -> Vec<String> {
    let mut syms: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for line in format!("{title}\n{brief}").lines().map(str::to_string) {
        for s in extract_line(&line) {
            if seen.insert(s.clone()) {
                syms.push(s);
            }
        }
    }
    if syms.is_empty() || scope.is_empty() {
        return Vec::new();
    }
    let files = scope_files(root, scope);
    if files.is_empty() {
        return Vec::new();
    }
    let texts = read_all(&files);
    let mut misses: Vec<String> = syms.into_iter().filter(|s| !present(&texts, s)).collect();
    if misses.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let shown = misses.len().min(10);
    for s in misses.drain(..shown) {
        out.push(format!("warning: brief names {s} — no match under scope"));
    }
    let rest = misses.len();
    if rest > 0 {
        out.push(format!(
            "warning: brief names {rest} more symbol(s) — no match under scope"
        ));
    }
    out
}

/// Backticked symbol tokens of one line that survive every skip rule, in order.
fn extract_line(line: &str) -> Vec<String> {
    if line.to_lowercase().contains("leave alone") {
        return Vec::new();
    }
    let mut out = Vec::new();
    // byte offsets of each backticked token in `line`, so the `new`/`add`
    // check sees what the line said before the token (the chunk creates it).
    let mut from = 0usize;
    for clause in super::chunk_scope::clauses(line) {
        if super::chunk_scope::is_check_line(clause) {
            continue;
        }
        for sentence in super::chunk_scope_skip::sentences(clause) {
            for tok in backticked(sentence) {
                let Some((pos, next)) = find_from(line, &tok, from) else {
                    continue;
                };
                from = next;
                if !is_symbol(&tok) {
                    continue;
                }
                if super::chunk_scope_skip::skipped(sentence, &tok) {
                    continue;
                }
                if has_new_add(&line[..pos]) {
                    continue;
                }
                if !out.iter().any(|o| o == &tok) {
                    out.push(tok);
                }
            }
        }
    }
    out
}

/// Contents of `` `...` `` spans of `s`, trimmed, in order.
fn backticked(s: &str) -> Vec<String> {
    let parts: Vec<&str> = s.split('`').collect();
    let mut out = Vec::new();
    let mut i = 1;
    while i < parts.len() {
        let t = parts[i].trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
        i += 2;
    }
    out
}

/// One identifier or `a::b` path: no spaces, not a path, not a flag.
fn is_symbol(tok: &str) -> bool {
    if tok.is_empty() || tok.contains(char::is_whitespace) {
        return false;
    }
    if tok.starts_with('-') {
        return false;
    }
    if tok.contains(['/', '\\', '.']) {
        return false;
    }
    let t = tok.strip_prefix("::").unwrap_or(tok);
    if t.is_empty() {
        return false;
    }
    t.split("::").all(|p| {
        let mut c = p.chars();
        c.next()
            .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
            && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
    })
}

/// `line[off..]` holding `tok`: its absolute start plus the next search offset.
fn find_from(line: &str, tok: &str, off: usize) -> Option<(usize, usize)> {
    line.get(off..)?
        .find(tok)
        .map(|r| (off + r, off + r + tok.len()))
}

/// The text before the token names the creation: a `new`/`add` word there.
fn has_new_add(before: &str) -> bool {
    before.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| {
        let w = w.to_ascii_lowercase();
        w == "new" || w == "add"
    })
}

/// Fixed-string search in every scope file; for `a::b` the last segment.
fn present(texts: &[String], sym: &str) -> bool {
    let needle = sym.rsplit("::").next().unwrap_or(sym);
    texts.iter().any(|t| t.contains(needle))
}

/// Files under the scope entries that exist on disk, deduped.
fn scope_files(root: &Path, scope: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for s in scope {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        let p = root.join(s);
        if p.is_file() {
            if !out.iter().any(|o| o == &p) {
                out.push(p);
            }
        } else if p.is_dir() {
            walk(&p, &mut out);
        }
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if out.len() > 2000 {
            return;
        }
        let p = e.path();
        if p.is_dir() {
            let skip = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == ".git" || n == "target");
            if !skip {
                walk(&p, out);
            }
        } else if p.is_file() && !out.iter().any(|o| o == &p) {
            out.push(p);
        }
    }
}

fn read_all(files: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for f in files {
        let Ok(b) = std::fs::read(f) else {
            continue;
        };
        if b.len() > 1_000_000 {
            continue;
        }
        out.push(String::from_utf8_lossy(&b).into_owned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{backticked, extract_line, has_new_add, is_symbol, present};

    #[test]
    fn only_identifiers_and_paths_count() {
        assert!(is_symbol("no_such_fn"));
        assert!(is_symbol("crate::no_such_fn"));
        assert!(is_symbol("_x1"));
        assert!(!is_symbol("src/x.rs"));
        assert!(!is_symbol("scripts/gate.sh"));
        assert!(!is_symbol("cargo test -p x"));
        assert!(!is_symbol("--scope"));
        assert!(!is_symbol("a b"));
        assert!(!is_symbol(""));
        assert!(!is_symbol("foo!"));
        assert!(!is_symbol("a:::b"));
    }

    #[test]
    fn last_segment_is_searched() {
        let texts = vec!["fn present_fn() {}".to_string()];
        assert!(present(&texts, "present_fn"));
        assert!(present(&texts, "crate::present_fn"));
        assert!(!present(&texts, "no_such_fn"));
        assert!(!present(&texts, "crate::no_such_fn"));
    }

    #[test]
    fn leave_alone_check_and_new_lines_are_skipped() {
        assert!(extract_line("leave alone `no_such_fn` here").is_empty());
        assert!(extract_line("Check: run `no_such_fn` green").is_empty());
        assert!(extract_line("Run: `no_such_fn`").is_empty());
        assert!(extract_line("Add helper `my_new_fn`").is_empty());
        assert!(extract_line("create new `my_new_fn` thing").is_empty());
        assert_eq!(extract_line("call `present_fn` now"), vec!["present_fn"]);
        assert!(extract_line("Do not touch `no_such_fn`").is_empty());
        assert!(extract_line("Then run `no_such_fn`").is_empty());
    }

    #[test]
    fn paths_and_commands_are_no_symbols() {
        assert!(extract_line("edit `src/x.rs` now").is_empty());
        assert!(extract_line("run `cargo test -p x` now").is_empty());
    }

    #[test]
    fn new_word_needs_boundaries() {
        assert!(!has_new_add("renew the "));
        assert!(has_new_add("add a "));
        assert!(has_new_add("Add helper "));
    }

    #[test]
    fn backticked_picks_odd_spans() {
        assert_eq!(
            backticked("edit `a` and `b::c`"),
            vec!["a".to_string(), "b::c".to_string()]
        );
    }
}
