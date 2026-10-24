//! Ask metrics over usage.jsonl (PLAN-fael-durable-log chunk 3a): the read
//! half of `asks` — transcript token recovery. Aggregation lives in
//! `fael-core::stats`; recording lives in `asks`; nothing here ever writes.

use serde::Serialize;
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

/// The latest model id in a Claude Code or Codex transcript: the model that
/// wrote the last round (run-telemetry t4). Tail-read like
/// `transcript_usage`, fail-open — a missing file, a non-transcript session
/// (opencode's RFC3339 hook session) or no model yet all read as `None`,
/// never an error. Both sources are client-reported (Anthropic/Codex write
/// their own transcripts); free text is never parsed.
pub(crate) fn transcript_model(session: &str) -> Option<String> {
    if session.is_empty() {
        return None;
    }
    let tail = tail_bytes(Path::new(session), 65536)?;
    tail.lines().rev().take(300).find_map(model_in_line)
}

/// One transcript line → its model id, Claude-shaped (`message.model`, else
/// `requestedModel`) or Codex-shaped (`payload.model` on `turn_context`,
/// else `collaboration_mode.settings.model`). Only an exact non-empty string —
/// a missing key reads as `None`, never a guess.
fn model_in_line(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let t = v.get("type")?.as_str()?;
    if t == "assistant" {
        let m = v
            .get("message")?
            .get("model")
            .and_then(serde_json::Value::as_str)
            .or_else(|| v.get("requestedModel").and_then(serde_json::Value::as_str));
        return m.filter(|m| !m.is_empty()).map(str::to_string);
    }
    if t == "turn_context" {
        let p = v.get("payload")?;
        let m = p
            .get("model")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                p.get("collaboration_mode")?
                    .get("settings")?
                    .get("model")?
                    .as_str()
            });
        return m.filter(|m| !m.is_empty()).map(str::to_string);
    }
    None
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

#[cfg(test)]
mod tests {
    use super::{RealTokens, model_in_line, transcript_model, transcript_usage, usage_in_line};

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

    fn claude_model_line(model: &str) -> String {
        serde_json::json!({"type": "assistant", "message": {"model": model}}).to_string()
    }

    #[test]
    fn model_reads_claude_message_model_latest_wins() {
        assert_eq!(
            model_in_line(&claude_model_line("claude-opus-5-5")).as_deref(),
            Some("claude-opus-5-5")
        );
        // requestedModel is the fallback when the message names none
        let l = serde_json::json!({"type": "assistant", "message": {}, "requestedModel": "r1"})
            .to_string();
        assert_eq!(model_in_line(&l).as_deref(), Some("r1"));
        // message.model wins over requestedModel
        let l = serde_json::json!({
            "type": "assistant",
            "message": {"model": "m1"},
            "requestedModel": "r1",
        })
        .to_string();
        assert_eq!(model_in_line(&l).as_deref(), Some("m1"));
        // usage lines name no model; junk never parses to one
        assert!(model_in_line(&usage_line(1, 0, 0, 1)).is_none());
        assert!(model_in_line(r#"{"type":"assistant"}"#).is_none());
        assert!(model_in_line("not json").is_none());
    }

    #[test]
    fn model_reads_codex_turn_context() {
        let l = serde_json::json!({
            "type": "turn_context",
            "payload": {"model": "gpt-6-luna"},
        })
        .to_string();
        assert_eq!(model_in_line(&l).as_deref(), Some("gpt-6-luna"));
        // nested collaboration setting is the fallback
        let l = serde_json::json!({
            "type": "turn_context",
            "payload": {"collaboration_mode": {"settings": {"model": "gpt-x"}}},
        })
        .to_string();
        assert_eq!(model_in_line(&l).as_deref(), Some("gpt-x"));
        assert!(model_in_line(r#"{"type":"turn_context","payload":{}}"#).is_none());
    }

    #[test]
    fn transcript_model_takes_the_latest_and_fails_open() {
        let dir = std::env::temp_dir().join(format!("fael-asks-{}", crate::core::ulid()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("transcript.jsonl");
        std::fs::write(
            &p,
            format!(
                "{}\n{}\n{}\n",
                claude_model_line("first"),
                serde_json::json!({"type": "user"}),
                claude_model_line("last"),
            ),
        )
        .unwrap();
        assert_eq!(
            transcript_model(p.to_str().unwrap()).as_deref(),
            Some("last")
        );
        assert!(transcript_model(dir.join("missing.jsonl").to_str().unwrap()).is_none());
        assert!(transcript_model("").is_none());
        // an RFC3339 hook session (opencode) is never a transcript
        assert!(transcript_model("2026-10-24T00:00:00.000Z").is_none());
    }
}
