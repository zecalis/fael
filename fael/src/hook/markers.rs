//! Bug markers (port of fapony's bug-markers.ts, std only): the phrases in
//! the assistant's text that make the stop hook ask for an issue row.

use crate::core;
use std::path::Path;

/// Free-text announcement phrases, never symptom words — a false fire costs a
/// Stop-hook block. Ported without a regex crate to keep the hook path std +
/// serde_json only; the behaviour matches on the common phrases.
const BUG_PHRASES_LATIN: [&str; 12] = [
    "found a bug",
    "found the bug",
    "found bug",
    "found a real bug",
    "found the real bug",
    "this is a bug",
    "that is a bug",
    "it is a bug",
    "it's a bug",
    "bug confirmed",
    "confirmed bug",
    "confirmed a bug",
];

/// Thai phrases are caseless — they match in the lowercased haystack too.
const BUG_PHRASES_THAI: [&str; 5] = ["เจอบั๊ก", "พบว่าเป็นบั๊ก", "เจอว่าเป็นบั๊ก", "บั๊กที่เจอ", "บั๊กที่พบ"];

const NEGATIONS: [&str; 7] = ["ไม่", "จะ", "ถ้า", "อาจ", "not", "no", "if"];

/// Problems short of a confirmed bug — something inconsistent, or expected to
/// break. Reporting these on the spot is the point (an agent has no reason to
/// stay quiet), so a false fire is the accepted cost: one block per session.
/// "might"/"อาจ" are the claim here, not a negation — only a flat denial
/// ("no mismatch", "ไม่มีความเสี่ยง") cancels.
const RISK_PHRASES: [&str; 18] = [
    "inconsistent",
    "inconsistency",
    "mismatch",
    "doesn't match",
    "does not match",
    "out of sync",
    "might break",
    "could break",
    "will break",
    "likely to break",
    "ไม่ตรงกัน",
    "ไม่สอดคล้อง",
    "ขัดแย้งกัน",
    "อาจพัง",
    "น่าจะพัง",
    "อาจมีปัญหา",
    "น่าจะมีปัญหา",
    "มีความเสี่ยง",
];

const RISK_NEGATIONS: [&str; 3] = ["ไม่", "not", "no"];

/// A matched marker: Strong = a confirmed-bug announcement (blocks without
/// an issue row), Weak = a risk/inconsistency mention (one-line note only,
///
/// never a block).
pub(crate) struct BugHit {
    pub(crate) marker: String,
    pub(crate) strong: bool,
}

/// A transcript match with its line timestamp — the adapter clears the signal
/// only with an issue row stamped at or after the match, never an older one.
pub(crate) struct TranscriptHit {
    pub(crate) marker: String,
    pub(crate) strong: bool,
    pub(crate) at_ms: i64,
}

/// The matched marker, or `None`. Code fences, `inline code` and `>` quotes
/// are dropped first — a phrase describing code is not a problem report.
/// Every match is checked against a negation window so "ไม่พบบั๊กใหม่
/// แต่เจอบั๊กที่ X" still fires on the second. The search runs on the
/// lowercased text throughout, so byte indices always belong to the string
/// they slice.
pub(crate) fn has_bug_marker(text: &str) -> Option<BugHit> {
    let lower = strip_quoted(text).to_lowercase();
    // phrase lists
    for p in BUG_PHRASES_LATIN.into_iter().chain(BUG_PHRASES_THAI) {
        if let Some(i) = lower.find(p)
            && !negated(&lower, i, &NEGATIONS)
        {
            return Some(BugHit {
                marker: lower[i..i + p.len()].to_string(),
                strong: true,
            });
        }
    }
    for p in RISK_PHRASES {
        if let Some(i) = lower.find(p)
            && !negated(&lower, i, &RISK_NEGATIONS)
        {
            return Some(BugHit {
                marker: p.to_string(),
                strong: false,
            });
        }
    }
    // `bug…:` — "**Bug (cause…):**" (same line, optional paren group)
    let mut from = 0;
    while let Some(i) = lower[from..].find("bug") {
        let i = from + i;
        if word_boundary(&lower, i, 3)
            && colon_after(&lower, i + 3)
            && !negated(&lower, i, &NEGATIONS)
        {
            return Some(BugHit {
                marker: "bug:".to_string(),
                strong: true,
            });
        }
        from = i + 3;
    }
    None
}

/// Drop fenced code blocks, `inline code` spans and `>` quote lines — what is
/// left is the assistant's own prose, the only part that can report a problem.
fn strip_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.split_inclusive('\n') {
        // a ``` run anywhere opens/closes a fenced block when unpaired on
        // the line — the marker line itself is never prose worth matching
        if line.contains("```") {
            if line.matches("```").count() % 2 == 1 {
                in_fence = !in_fence;
            }
            continue;
        }
        if in_fence || line.trim_start().starts_with('>') {
            continue;
        }
        out.push_str(&strip_inline_code(line));
    }
    out
}

/// Drop `code` spans on one line; the tail after an unpaired tick is prose.
fn strip_inline_code(line: &str) -> String {
    let parts: Vec<&str> = line.split('`').collect();
    let mut out = String::with_capacity(line.len());
    for (i, part) in parts.iter().enumerate() {
        let tail_after_unpaired = parts.len().is_multiple_of(2) && i == parts.len() - 1;
        if i % 2 == 0 || tail_after_unpaired {
            out.push_str(part);
        }
    }
    out
}

fn word_boundary(s: &str, i: usize, len: usize) -> bool {
    let b = s.as_bytes();
    let left = i == 0 || !b[i - 1].is_ascii_alphanumeric();
    let right = b.get(i + len).is_none_or(|c| !c.is_ascii_alphanumeric());
    left && right
}

fn colon_after(s: &str, mut i: usize) -> bool {
    let b = s.as_bytes();
    while b.get(i).is_some_and(|c| *c == b' ' || *c == b'\t') {
        i += 1;
    }
    if b.get(i) == Some(&b'(') {
        // skip to the closing paren on this line
        while let Some(c) = b.get(i) {
            i += 1;
            if *c == b')' {
                break;
            }
            if *c == b'\n' {
                return false;
            }
        }
        while b.get(i).is_some_and(|c| *c == b' ' || *c == b'\t') {
            i += 1;
        }
    }
    // a colon before the line ends
    s[i..]
        .split('\n')
        .next()
        .is_some_and(|l| l.trim_end().ends_with(':'))
}

/// The ~15 chars before the match end in a negation word.
fn negated(lower: &str, i: usize, negations: &[&str]) -> bool {
    let before: String = lower[..i]
        .chars()
        .rev()
        .take(15)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let t = before.trim_end().to_lowercase();
    negations.iter().any(|n| {
        t.ends_with(n)
            && (n.chars().all(|c| !c.is_ascii_alphabetic())
                || t[..t.len() - n.len()]
                    .chars()
                    .last()
                    .is_none_or(|c| !c.is_alphabetic()))
    })
}

/// Last ≤200 KB of a small transcript (nothing over 10 MB) — the hook must
/// not stall turn-end. Skips a partial first line.
fn read_tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    if size > 10 * 1024 * 1024 {
        return None;
    }
    let tail = size.min(200 * 1024);
    f.seek(SeekFrom::Start(size - tail)).ok()?;
    let mut buf = vec![0u8; tail as usize];
    f.read_exact(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    if tail < size {
        text.split_once('\n').map(|(_, rest)| rest.to_string())
    } else {
        Some(text)
    }
}

/// Scan assistant text in a Claude transcript tail for a bug marker — only
/// lines after the latest user message (a plan written an hour ago must not
/// block this turn). Returns the match with its line timestamp (fallback:
/// the session start, when the line carries none).
pub(crate) fn bug_signal_from_transcript(path: &Path, since_ms: i64) -> Option<TranscriptHit> {
    let text = read_tail(path)?;
    // (is_user, line ms, text blocks) in file order
    let mut msgs: Vec<(bool, i64, Vec<String>)> = vec![];
    for line in text.split('\n') {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let m = &v["message"];
        let role = m["role"].as_str().unwrap_or("");
        if role != "assistant" && role != "user" {
            continue;
        }
        // Claude Code stamps each line at the top level, not inside `message`
        let at_ms = v["timestamp"]
            .as_str()
            .and_then(core::ts_ms)
            .unwrap_or(since_ms);
        if at_ms < since_ms {
            continue;
        }
        let mut texts = vec![];
        for b in m["content"].as_array().into_iter().flatten() {
            if b["type"] == "text"
                && let Some(t) = b["text"].as_str()
            {
                texts.push(t.to_string());
            }
        }
        msgs.push((role == "user", at_ms, texts));
    }
    let after = msgs
        .iter()
        .rposition(|(user, _, _)| *user)
        .map(|i| i + 1)
        .unwrap_or(0);
    for (_, at_ms, texts) in msgs.iter().skip(after).filter(|m| !m.0) {
        for t in texts {
            if let Some(hit) = has_bug_marker(t) {
                return Some(TranscriptHit {
                    marker: hit.marker,
                    strong: hit.strong,
                    at_ms: *at_ms,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::has_bug_marker;

    #[test]
    fn strong_blocks_weak_notes() {
        let hit = has_bug_marker("I found a bug in login").unwrap();
        assert!(hit.strong, "{}", hit.marker);
        let hit = has_bug_marker("doc กับโค้ดไม่ตรงกัน").unwrap();
        assert!(!hit.strong, "{}", hit.marker);
    }

    #[test]
    fn quoted_code_never_signals() {
        for quiet in [
            "```\nI found a bug in login\n```",
            "run `found a bug` to reproduce",
            "> I found a bug in login",
            "> doc กับโค้ดไม่ตรงกัน",
            "```\nconfig and schema are out of sync\n```",
        ] {
            assert!(has_bug_marker(quiet).is_none(), "{quiet}");
        }
        // prose around code still fires
        let hit = has_bug_marker("looks off:\n```\nlet x = 1;\n```\nI found a bug below").unwrap();
        assert!(hit.strong, "{}", hit.marker);
    }

    #[test]
    fn markers_catch_bugs_and_risks_not_denials() {
        for hit in [
            "I found a bug in login",
            "doc กับโค้ดไม่ตรงกัน",
            "config and schema are out of sync",
            "this might break the importer",
            "ตรงนี้อาจมีปัญหาตอน merge",
        ] {
            assert!(has_bug_marker(hit).is_some(), "{hit}");
        }
        for miss in [
            "no bug found",
            "no mismatch left",
            "ไม่มีความเสี่ยง",
            "ถ้าเจอบั๊กให้บอก",
            "all tests pass",
        ] {
            assert!(has_bug_marker(miss).is_none(), "{miss}");
        }
    }
}
