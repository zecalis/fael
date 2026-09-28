//! Ask metrics over usage.jsonl (PLAN-fael-durable-log chunk 3a): the read
//! half of `asks` — transcript token recovery plus the counts `stats` shows.
//! Recording lives in `asks`; nothing here ever writes.

use super::asks::{ASK_BLOCK, ASK_REJECT, ASK_WARN};
use crate::core;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Real tokens of one assistant round, straight from the transcript's `usage`
/// object — input-side split kept because cache reads are cheap but still
/// count toward the limit. Field names mirror Claude's, so the mapping is
/// greppable in both directions.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
pub(crate) struct RealTokens {
    pub input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub output_tokens: u64,
}

/// The latest assistant `usage` in a Claude Code transcript: the round that
/// just ended when this hook fired. Tail-read (transcripts run to MBs),
/// fail-open — a missing file, a non-Claude transcript (codex, opencode) or
/// no `usage` yet all read as `None`, never an error.
pub(crate) fn transcript_usage(session: &str) -> Option<RealTokens> {
    if session.is_empty() {
        return None;
    }
    let tail = tail_bytes(Path::new(session), 65536)?;
    tail.lines().rev().take(300).find_map(usage_in_line)
}

/// One transcript line → its `usage`, Claude-shaped (`message.usage` or a
/// top-level `usage`). Only `input_tokens` is required — a partial object
/// still beats no data, the missing sides read 0.
fn usage_in_line(line: &str) -> Option<RealTokens> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let u = v.get("usage").or_else(|| v.get("message")?.get("usage"))?;
    Some(RealTokens {
        input_tokens: u.get("input_tokens")?.as_u64()?,
        cache_creation_input_tokens: usage_part(u, "cache_creation_input_tokens"),
        cache_read_input_tokens: usage_part(u, "cache_read_input_tokens"),
        output_tokens: usage_part(u, "output_tokens"),
    })
}

fn usage_part(u: &serde_json::Value, k: &str) -> u64 {
    u.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0)
}

/// The last `n` bytes of a file as a string (lossy — a torn multibyte char at
/// the cut never matters: that line just fails to parse and is skipped).
fn tail_bytes(path: &Path, n: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(n))).ok()?;
    // read bytes, then decode lossily: read_to_string errors when the cut
    // lands inside a multibyte char (routine with Thai), which would drop
    // every usage line in the tail — markers.rs:225 does the same job this way
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Ask counts in fixed order (reject, stop-block, warning): (count, bytes).
/// Plain pushes carry no `ask` and never count here.
pub(crate) fn ask_totals(rows: &[serde_json::Value]) -> Vec<(&'static str, usize, u64)> {
    [ASK_REJECT, ASK_BLOCK, ASK_WARN]
        .into_iter()
        .map(|ask| {
            let (mut n, mut b) = (0usize, 0u64);
            for r in rows {
                if r.get("ask").and_then(|a| a.as_str()) == Some(ask) {
                    n += 1;
                    b += r
                        .get("bytes")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                }
            }
            (ask, n, b)
        })
        .collect()
}

/// Repeat stop-blocks: a block that follows another block in the same session
/// with no row filed between them — the agent paid for a round and still had
/// nothing to show. Needs the session on the rows (hooks only); session-less
/// rows never count. Row times come from the repo logs; rows by anyone count
/// (per-session attribution would need `session` on rows).
pub(crate) fn repeat_blocks(
    blocks: &[(String, String, i64, String)],
    logs: &HashMap<String, core::Log>,
) -> usize {
    let mut by_session: HashMap<&str, Vec<(i64, &str)>> = HashMap::new();
    for (repo, _, ms, session) in blocks {
        if !session.is_empty() {
            by_session.entry(session).or_default().push((*ms, repo));
        }
    }
    let mut repeat = 0usize;
    for times in by_session.values() {
        let mut ts: Vec<(i64, &str)> = times.clone();
        ts.sort();
        for w in ts.windows(2) {
            let [(prev, _), (cur, repo)] = w else {
                continue;
            };
            let gap_has_row = logs.get(*repo).is_some_and(|log| {
                log.rows
                    .iter()
                    .any(|r| core::ts_ms(&r.ts).is_some_and(|t| t > *prev && t <= *cur))
            });
            if !gap_has_row {
                repeat += 1;
            }
        }
    }
    repeat
}

/// Rows filed at or after `since_ms` — the denominator for "rows that took
/// their own round after a block vs rows that rode along".
pub(crate) fn added_since(log: &core::Log, since_ms: i64) -> usize {
    log.rows
        .iter()
        .filter(|r| core::ts_ms(&r.ts).is_some_and(|t| t >= since_ms))
        .count()
}

/// (rows, rows with Thai): one global dedup by id — repos in one clone share
/// the journal, so the same row must not count twice. Thai = U+0E00–U+0E7F in
/// title or text; the chunk-6 detector is wider, this is the baseline share.
pub(crate) fn thai_share(logs: &HashMap<String, core::Log>) -> (usize, usize) {
    fn thai(s: &str) -> bool {
        s.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c))
    }
    let mut seen = HashSet::new();
    let (mut n, mut thai_n) = (0usize, 0usize);
    for log in logs.values() {
        for r in &log.rows {
            if !seen.insert(r.id.as_str()) {
                continue;
            }
            n += 1;
            if thai(&r.text) || r.title.as_deref().is_some_and(thai) {
                thai_n += 1;
            }
        }
    }
    (n, thai_n)
}

/// Mean real-token cost of the round after a stop-block: each block attributes
/// the next same-session usage row that carries `real_tokens` (the round its
/// block caused — "the cost of the round following a block", never "tokens
/// fael used"). Returns (samples, avg input, avg cache-create, avg cache-read,
/// avg output); zero samples when no transcript had `usage`.
pub(crate) fn post_block_cost(rows: &[serde_json::Value]) -> (usize, u64, u64, u64, u64) {
    let mut ev: Vec<(i64, &str, bool, Option<RealTokens>)> = vec![];
    for r in rows {
        let (Some(ts), Some(session)) = (
            r.get("ts").and_then(|t| t.as_str()).and_then(core::ts_ms),
            r.get("session").and_then(|s| s.as_str()),
        ) else {
            continue;
        };
        if session.is_empty() {
            continue;
        }
        ev.push((
            ts,
            session,
            r.get("ask").and_then(|a| a.as_str()) == Some(ASK_BLOCK),
            real_in(r),
        ));
    }
    ev.sort_by_key(|e| e.0);
    let mut pending: HashMap<&str, bool> = HashMap::new();
    let (mut n, mut sums) = (0usize, [0u64; 4]);
    for (_, session, block, real) in ev {
        if block {
            pending.insert(session, true);
        } else if let Some(t) = real
            && pending.remove(session).is_some()
        {
            n += 1;
            sums[0] += t.input_tokens;
            sums[1] += t.cache_creation_input_tokens;
            sums[2] += t.cache_read_input_tokens;
            sums[3] += t.output_tokens;
        }
    }
    if n == 0 {
        return (0, 0, 0, 0, 0);
    }
    let avg = |i: usize| sums[i] / n as u64;
    (n, avg(0), avg(1), avg(2), avg(3))
}

/// A usage row's `real_tokens`, when the hook found transcript `usage`.
fn real_in(r: &serde_json::Value) -> Option<RealTokens> {
    let u = r.get("real_tokens")?;
    Some(RealTokens {
        input_tokens: u.get("input_tokens")?.as_u64()?,
        cache_creation_input_tokens: usage_part(u, "cache_creation_input_tokens"),
        cache_read_input_tokens: usage_part(u, "cache_read_input_tokens"),
        output_tokens: usage_part(u, "output_tokens"),
    })
}

#[cfg(test)]
mod tests {
    use super::{RealTokens, post_block_cost, transcript_usage, usage_in_line};

    fn usage_line(input: u64, create: u64, read: u64, output: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {"usage": {
                "input_tokens": input,
                "cache_creation_input_tokens": create,
                "cache_read_input_tokens": read,
                "output_tokens": output,
            }},
        })
        .to_string()
    }

    #[test]
    fn usage_line_reads_claude_shape() {
        let t = usage_in_line(&usage_line(100, 20000, 30000, 50)).unwrap();
        assert_eq!(
            t,
            RealTokens {
                input_tokens: 100,
                cache_creation_input_tokens: 20000,
                cache_read_input_tokens: 30000,
                output_tokens: 50,
            }
        );
        assert!(usage_in_line(r#"{"type":"user","message":"hi"}"#).is_none());
        assert!(usage_in_line("not json").is_none());
    }

    #[test]
    fn transcript_takes_the_latest_usage() {
        let dir = std::env::temp_dir().join(format!("fael-asks-{}", crate::core::ulid()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("transcript.jsonl");
        std::fs::write(
            &p,
            format!("{}\n{}\n", usage_line(10, 0, 0, 1), usage_line(20, 0, 0, 2)),
        )
        .unwrap();
        assert_eq!(
            transcript_usage(p.to_str().unwrap()).map(|t| t.input_tokens),
            Some(20)
        );
        assert!(transcript_usage(dir.join("missing.jsonl").to_str().unwrap()).is_none());
        assert!(transcript_usage("").is_none());
    }

    #[test]
    fn transcript_tail_cut_mid_multibyte_still_reads_usage() {
        let dir = std::env::temp_dir().join(format!("fael-asks-{}", crate::core::ulid()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("transcript.jsonl");
        // >64 KiB of Thai (3 bytes each) so the 64 KiB tail starts inside a
        // char — read_to_string would fail on the partial char and read None
        let mut s = "ก".repeat(70_000 / 3);
        s.push('\n');
        s.push_str(&usage_line(7, 0, 0, 1));
        s.push('\n');
        std::fs::write(&p, s).unwrap();
        assert_eq!(
            transcript_usage(p.to_str().unwrap()).map(|t| t.input_tokens),
            Some(7)
        );
    }

    #[test]
    fn post_block_cost_joins_block_to_next_real() {
        let row = |ts: &str, ask: &str, session: &str, real: bool| {
            let mut v = serde_json::json!({"ts": ts, "session": session, "ask": ask});
            if real {
                v["real_tokens"] = serde_json::json!({"input_tokens": 1000,
                    "cache_creation_input_tokens": 2000, "cache_read_input_tokens": 3000,
                    "output_tokens": 100});
            }
            v
        };
        let rows = vec![
            row("2026-09-28T00:00:01Z", "stop-block", "s1", false),
            row("2026-09-28T00:00:02Z", "", "s1", true),
            row("2026-09-28T00:00:03Z", "stop-block", "s1", false),
            row("2026-09-28T00:00:04Z", "", "s1", false),
            row("2026-09-28T00:00:05Z", "stop-block", "s2", false),
        ];
        // one sample: the first block's next real row; the dangling blocks
        // (no later real row, other session) contribute nothing
        assert_eq!(post_block_cost(&rows), (1, 1000, 2000, 3000, 100));
        assert_eq!(post_block_cost(&[]), (0, 0, 0, 0, 0));
    }
}
