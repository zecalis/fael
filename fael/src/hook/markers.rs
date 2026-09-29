//! Bug markers behind `.fael/config.toml` `[lang]` (PLAN-fael-languages
//! chunk 2): every phrase lives in a `core::lang` pack, so this adapter only
//! reads the transcript tail and calls `core::lang::marker_hit` with the
//! packs the repo selected. `marker = []` switches the bug rule off entirely.

use crate::core;
use std::path::Path;

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

/// The packs behind `cfg.lang_marker`, in repo order — unknown names never
/// reach here (`Config::from_toml` rejects them), so this filters defensively.
fn packs(cfg: &core::Config) -> Vec<&'static core::lang::Lang> {
    cfg.lang_marker
        .iter()
        .filter_map(|n| core::lang::by_name(n))
        .collect()
}

/// The matched marker, or `None` — `core::lang::marker_hit` with this repo's
/// packs. Matching semantics (quote stripping, negation window, the `bug…:`
/// rule) live in core; the phrase unit tests moved to
/// `fael-core/tests/lang.rs` with them.
pub(crate) fn has_bug_marker(text: &str, cfg: &core::Config) -> Option<BugHit> {
    core::lang::marker_hit(text, &packs(cfg), &[]).map(|h| BugHit {
        marker: h.marker,
        strong: h.strong,
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
pub(crate) fn bug_signal_from_transcript(
    path: &Path,
    since_ms: i64,
    cfg: &core::Config,
) -> Option<TranscriptHit> {
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
            if let Some(hit) = has_bug_marker(t, cfg) {
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
