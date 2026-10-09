//! ReportFindings → issue (PLAN-fael-experience-loop chunk 2). A review's
//! findings are structured tool input, so nothing is inferred: each finding's
//! `file` and `summary` become one ready `fael add issue` line, `MAX` per
//! session, each finding (file + line) once. fael only offers the command;
//! the agent files it or not.

use super::protocol::{Event, Reply, ctx};
use super::say::{Kind, Line, Outbox};
use serde_json::Value;

/// Findings offered per session, so a long review is not a wall of lines.
const MAX: usize = 5;
/// Longest summary put in the command; the agent can edit it before filing.
const SUMMARY_MAX: usize = 140;
const MARK: &str = "~finding:";

/// The summary as one single-quoted shell word: whitespace collapsed, cut at
/// `SUMMARY_MAX` chars, every char kept — `$`, backtick and `\` are literal in
/// single quotes, and a `'` is closed, escaped and reopened (`'\''`).
fn summary(s: &str) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = match flat.char_indices().nth(SUMMARY_MAX) {
        Some((i, _)) => format!("{}…", flat[..i].trim_end()),
        None => flat,
    };
    format!("'{}'", cut.replace('\'', r"'\''"))
}

/// One `Finding` line per fresh finding of `input` (`findings[]`), at most
/// `MAX` less those the session was already offered.
fn lines(input: &Value, out: &Outbox) -> Vec<Line> {
    let offered = out.seen().lines().filter(|l| l.starts_with(MARK)).count();
    let mut keys: Vec<String> = vec![];
    let found = input["findings"].as_array().into_iter().flatten();
    found
        .filter_map(|f| {
            let (file, text) = (f["file"].as_str()?.trim(), summary(f["summary"].as_str()?));
            let line = f["line"].as_i64().unwrap_or(0);
            let key = format!("{MARK}{file}:{line}");
            if file.is_empty() || text == "''" || out.has(&key) || keys.contains(&key) {
                return None;
            }
            keys.push(key);
            Some(Line {
                kind: Kind::Finding {
                    file: file.into(),
                    line,
                },
                text: format!(
                    "fael: review finding on {file} — file it: `fael add issue {text} --files {file}`\n"
                ),
            })
        })
        .take(MAX.saturating_sub(offered))
        .collect()
}

/// The reply for one ReportFindings call: silence when it holds nothing fresh.
pub(crate) fn report_reply(e: &Event, input: &Value) -> Reply {
    let Some(c) = ctx(e) else {
        return Reply::default();
    };
    let seen = (!c.session.is_empty())
        .then(|| {
            super::state::lock_seen(&super::state::seen_path(&c.session, &c.agent, &c.repo.root))
        })
        .flatten();
    let mut out = Outbox::open(seen);
    for l in lines(input, &out) {
        out.say(l);
    }
    let r = out.reply();
    if let Some(context) = r.context() {
        let meta = super::asks::UsageMeta {
            said: r.said(),
            ..super::asks::hook_meta(&c, None, true)
        };
        super::usage::record_usage(&c.client, "review", &c.repo.root, context, &[], &meta);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_keeps_every_char_in_one_single_quoted_word() {
        assert_eq!(summary("a \"b\"\n  `c` $d \\e"), r#"'a "b" `c` $d \e'"#);
        assert_eq!(summary("it's"), r"'it'\''s'");
        let long = "x".repeat(SUMMARY_MAX + 10);
        assert_eq!(summary(&long).chars().count(), SUMMARY_MAX + 3);
        assert!(summary(&long).ends_with("…'"));
    }

    #[test]
    fn a_thai_summary_is_cut_on_a_char_boundary() {
        let long = "ผิด".repeat(SUMMARY_MAX);
        assert_eq!(summary(&long).chars().count(), SUMMARY_MAX + 3);
    }

    #[test]
    fn a_malformed_report_says_nothing() {
        let out = Outbox::open(None);
        for input in [
            json!({}),
            json!({"findings": "x"}),
            json!({"findings": [{"file": "a.rs"}, {"summary": "x"}, {"file": 3, "summary": 4}]}),
        ] {
            assert!(lines(&input, &out).is_empty(), "{input}");
        }
    }

    #[test]
    fn a_finding_is_offered_once_and_five_per_session() {
        let f = |file: &str, line: i64| json!({"file": file, "summary": "broken", "line": line});
        let findings: Vec<Value> = (0..7).map(|i| f(&format!("a{i}.rs"), 1)).collect();
        let input = json!({ "findings": findings });
        let out = Outbox::open(None);
        let got = lines(&input, &out);
        assert_eq!(got.len(), MAX);
        assert!(
            got[0]
                .text
                .contains("`fael add issue 'broken' --files a0.rs`")
        );
        // a file-less or summary-less finding says nothing; a repeat is one
        let thin = json!({"findings": [{"file": "", "summary": "x"}, f("b.rs", 1), f("b.rs", 1)]});
        assert_eq!(lines(&thin, &out).len(), 1);
    }
}
