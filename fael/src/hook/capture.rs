//! Capture in reply (PLAN-fael-dev-adoption chunk 1): the agent ends its last
//! message with `fael decision: …` / `fael issue: …` / `fael note: …` lines,
//! each carrying a required `[files: a,b]`, and the Stop hook files them
//! through the same write path as `fael add`. Nothing here starts a turn —
//! a rejected line becomes one hint on the next push.
//!
//! Only lines the agent chose to write: exact prefix at column 0, outside a
//! code fence, in the last assistant message. Prose is never interpreted and
//! scope is never inferred from the session's edits.

use super::asks::append_row;
use super::markers::read_tail;
use super::protocol::Ctx;
use super::state::{hint_path, note_seen, now_rfc3339, session_key};
use crate::core;
use crate::write::{AddOpts, add_row};
use std::path::Path;

const KINDS: [&str; 3] = ["decision", "issue", "note"];

/// One well-formed capture line, ready for the `add` path.
#[derive(Debug, PartialEq)]
pub(super) struct Line {
    pub(super) kind: &'static str,
    pub(super) text: String,
    pub(super) files: Vec<String>,
}

/// A line that carried the prefix but cannot be filed; `why` is safe to show
/// (it never repeats the line, so a leaked secret is not echoed).
#[derive(Debug, PartialEq)]
pub(super) struct Reject {
    pub(super) why: String,
}

/// What a stop's collector did — the caller refreshes its log and skips the
/// bug hint when an issue was filed.
#[derive(Default)]
pub(super) struct Filed {
    pub(super) stored: usize,
    pub(super) issue: bool,
}

/// Every `fael <kind>: <text> [files: …]` line of `text`, in order. Pure.
pub(super) fn parse(text: &str) -> Vec<Result<Line, Reject>> {
    let mut fence: Option<(char, usize)> = None;
    let mut out = vec![];
    for line in text.lines() {
        if let Some(open) = fence_of(line) {
            match fence {
                None => fence = Some(open),
                Some((c, n)) if open.0 == c && open.1 >= n && closes(line) => fence = None,
                Some(_) => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let hit = KINDS.iter().find_map(|k| {
            Some((
                *k,
                line.strip_prefix("fael ")?
                    .strip_prefix(k)?
                    .strip_prefix(": ")?,
            ))
        });
        if let Some((kind, rest)) = hit {
            out.push(one(kind, rest));
        }
    }
    out
}

/// The fence char and run length when `line` opens or closes a fence. A
/// backtick run with another backtick after it is inline code, not a fence
/// (CommonMark: a backtick fence's info string holds no backtick).
fn fence_of(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3 && !(c == '`' && t[n..].contains('`'))).then_some((c, n))
}

/// A closing fence carries nothing after its run.
fn closes(line: &str) -> bool {
    let t = line.trim();
    t.trim_start_matches('`').is_empty() || t.trim_start_matches('~').is_empty()
}

fn one(kind: &'static str, rest: &str) -> Result<Line, Reject> {
    let bad = |why: &str| {
        Err(Reject {
            why: format!("fael {kind}: {why}"),
        })
    };
    let rest = rest.trim_end();
    let Some(i) = rest.rfind("[files:").filter(|_| rest.ends_with(']')) else {
        return bad("line has no [files: a,b] at its end");
    };
    let files: Vec<String> = rest[i + "[files:".len()..rest.len() - 1]
        .split(',')
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    if files.is_empty() {
        return bad("[files: …] names no file");
    }
    let text = rest[..i].trim();
    if text.is_empty() {
        return bad("line has no text before [files: …]");
    }
    Ok(Line {
        kind,
        text: text.to_string(),
        files,
    })
}

/// The last assistant message of a Claude transcript: the text after the
/// latest user-role line (a tool result is one too, so text written before a
/// tool call never counts). Nothing flushed yet = `None`, never an older line.
pub(super) fn transcript_reply(path: &Path) -> Option<String> {
    let mut reply: Vec<String> = vec![];
    for line in read_tail(path)?.split('\n') {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["isSidechain"] == true {
            continue;
        }
        match v["message"]["role"].as_str() {
            Some("user") => reply.clear(),
            Some("assistant") => reply.extend(
                v["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str().map(String::from)),
            ),
            _ => {}
        }
    }
    (!reply.is_empty()).then(|| reply.join("\n"))
}

/// File every capture line of `reply` through `add`, count each in usage, and
/// stash one hint for the rejects. The same lines twice in one session (an
/// idle that fires again) are skipped.
pub(super) fn collect(c: &Ctx, reply: &str) -> Filed {
    let lines = parse(reply);
    let mut filed = Filed::default();
    if lines.is_empty() || seen_before(c, reply) {
        return filed;
    }
    let mut why: Vec<String> = vec![];
    for l in lines {
        match l.and_then(|l| {
            file(c, &l)
                .map(|id| (l.kind, id))
                .map_err(|why| Reject { why })
        }) {
            Ok((kind, id)) => {
                usage(c, "stored", Some(&id));
                // a sub-agent's row is news to the parent: it only got a summary
                if c.agent.is_empty() {
                    note_seen(&c.session, &c.repo.root, &[&id]);
                }
                super::tally::note(&c.session, &c.repo.root, "filed", &[&id]);
                filed.stored += 1;
                filed.issue |= kind == "issue";
            }
            Err(r) => {
                usage(c, "rejected", None);
                why.push(r.why);
            }
        }
    }
    if let Some(first) = why.first() {
        stash_hint(c, why.len(), first);
    }
    filed
}

fn file(c: &Ctx, l: &Line) -> Result<String, String> {
    let opts = AddOpts {
        key: None,
        to: None,
        title: None,
        revisit: None,
        urgent: core::Urgent::Unset,
        supersedes: None,
        force: false,
    };
    // files are never empty here (parse rejects it), so `add` cannot infer them
    add_row(&c.repo, l.kind, &l.text, &l.files, opts).map(|(row, ..)| row.id)
}

/// True when this session already handled exactly these capture lines; else
/// records them. ponytail: last set only, so a line repeated in a later,
/// different reply files again — add a per-line set if that shows in stats.
fn seen_before(c: &Ctx, reply: &str) -> bool {
    if c.session.is_empty() {
        return false;
    }
    let sig = session_key(
        &reply
            .lines()
            .filter(|l| KINDS.iter().any(|k| l.starts_with(&format!("fael {k}: "))))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let key = session_key(&format!("{}\0{}", c.session, c.repo.root.to_string_lossy()));
    let path = super::state::state_dir()
        .join("sessions")
        .join(format!("{key}.capture"));
    if std::fs::read_to_string(&path).is_ok_and(|s| s == sig) {
        return true;
    }
    if std::fs::create_dir_all(path.parent().unwrap_or(&c.repo.root)).is_ok() {
        let _ = std::fs::write(&path, sig);
    }
    false
}

fn usage(c: &Ctx, what: &str, row: Option<&str>) {
    let mut v = serde_json::json!({
        "ts": now_rfc3339().unwrap_or_default(),
        "repo": c.repo.root.to_string_lossy(),
        "client": c.client,
        "event": "capture",
        "capture": what,
        "bytes": 0,
        "est_tokens": 0,
    });
    if !c.session.is_empty() {
        v["session"] = c.session.as_str().into();
    }
    if let Some(id) = row {
        v["row"] = id.into(); // not `ids`: a capture is no push
    }
    append_row(v);
}

/// One line for the next push — the first reason, the count of rejects.
fn stash_hint(c: &Ctx, n: usize, why: &str) {
    if c.session.is_empty() {
        return;
    }
    let why = why
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches("rejected: ");
    // the hint lands on the parent's next push: a sub-agent's lines are not its own
    let whose = if c.agent.is_empty() {
        "your last reply"
    } else {
        "a sub-agent's last reply"
    };
    let path = hint_path(&c.session, &c.repo.root);
    if std::fs::create_dir_all(path.parent().unwrap_or(&c.repo.root)).is_ok() {
        let _ = std::fs::write(
            path,
            format!(
                "{n} `fael <kind>:` line(s) in {whose} were not filed — {why}. Fix the line, or file it with `fael add`.\n"
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(k: &'static str, t: &str, f: &[&str]) -> Result<Line, Reject> {
        Ok(Line {
            kind: k,
            text: t.into(),
            files: f.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[test]
    fn the_three_forms_parse_with_files() {
        let text = "done, tests pass\n\nfael decision: keys include the tenant [files: src/cache.rs]\nfael issue: retry has no backoff [files: a.rs, b.rs]\nfael note: stopped after the parser [files: plan:x]\n";
        assert_eq!(
            parse(text),
            vec![
                ok("decision", "keys include the tenant", &["src/cache.rs"]),
                ok("issue", "retry has no backoff", &["a.rs", "b.rs"]),
                ok("note", "stopped after the parser", &["plan:x"]),
            ]
        );
    }

    #[test]
    fn only_a_bare_line_start_counts() {
        for t in [
            "see fael decision: x [files: a]",
            "- fael decision: x [files: a]",
            "  fael decision: x [files: a]",
            "Fael decision: x [files: a]",
            "fael decision - x [files: a]",
            "fael decision:x [files: a]",
            "fael decisions: x [files: a]",
            "fael find: x [files: a]",
            "> fael note: x [files: a]",
        ] {
            assert!(parse(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn fenced_lines_are_skipped_and_the_fence_closes() {
        let text = "```cargo test```\nfael note: inline [files: i]\n```\nfael note: quoted [files: a]\n```\nfael note: real [files: b]\n~~~sh\nfael note: quoted [files: a]\n~~~\n````\n```\nfael note: nested [files: a]\n```\n````\nfael note: after [files: c]";
        assert_eq!(
            parse(text),
            vec![
                ok("note", "inline", &["i"]),
                ok("note", "real", &["b"]),
                ok("note", "after", &["c"])
            ]
        );
    }

    #[test]
    fn missing_or_empty_files_and_text_are_rejects() {
        for t in [
            "fael note: no files at all",
            "fael note: empty [files: ]",
            "fael note: commas only [files: , ,]",
            "fael note: [files: a]",
            "fael note: files not last [files: a] trailing",
            "fael note: unclosed [files: a",
        ] {
            let p = parse(t);
            assert_eq!(p.len(), 1, "{t}");
            assert!(p[0].is_err(), "{t}: {p:?}");
        }
    }

    #[test]
    fn a_reject_never_echoes_the_line() {
        let p = parse("fael note: token sk-abc123 leaks");
        assert!(!p[0].as_ref().unwrap_err().why.contains("sk-abc123"));
    }

    #[test]
    fn transcript_reply_is_the_text_after_the_last_user_line() {
        let d = std::env::temp_dir().join(format!("fael-cap-{}", core::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let t = d.join("t.jsonl");
        let line = |role: &str, text: &str| {
            serde_json::json!({"message": {"role": role, "content": [{"type": "text", "text": text}]}}).to_string()
        };
        let body = [
            line("assistant", "fael note: old [files: a]"),
            line("user", "tool result"),
            line("assistant", "fael note: mid-turn [files: a]"),
            line("user", "tool result"),
            line("assistant", "fael note: final [files: a]"),
        ]
        .join("\n");
        std::fs::write(&t, body).unwrap();
        assert_eq!(transcript_reply(&t).unwrap(), "fael note: final [files: a]");
        // a user line last = the final message is not flushed yet
        std::fs::write(&t, [line("assistant", "x"), line("user", "y")].join("\n")).unwrap();
        assert_eq!(transcript_reply(&t), None);
    }

    #[test]
    fn a_long_transcript_still_yields_its_last_message() {
        let d = std::env::temp_dir().join(format!("fael-cap-big-{}", core::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let t = d.join("t.jsonl");
        let filler = serde_json::json!({"message": {"role": "user", "content": "x".repeat(1000)}});
        let last = serde_json::json!({"message": {"role": "assistant", "content": [
            {"type": "text", "text": "fael note: at the end [files: a]"}]}});
        let mut body = format!("{filler}\n").repeat(11_000); // ~11 MB
        body.push_str(&last.to_string());
        std::fs::write(&t, body).unwrap();
        assert_eq!(
            transcript_reply(&t).unwrap(),
            "fael note: at the end [files: a]"
        );
    }
}
