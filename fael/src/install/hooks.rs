//! Per-client wiring: MCP entries, hook tables, the in-process plugin.
//!
//! Thin entry only — the clients live in `install/`:
//! `hooks` (the shared hooks-JSON shape), `claude`, `codex`, `opencode`.

use super::Ctx;
use serde_json::{Map, Value, json};
use std::path::Path;

// --- claude / codex hooks: the same {hooks: {Event: [{matcher?, hooks: [{type, command, timeout}]}]}} ---

/// Seconds a fael hook may run before the client kills it. Unset, a hung hook
/// holds the turn for the client's default (Claude Code: 600s); fael's hooks
/// take milliseconds (docs/integrate.md), so 10s only ever ends a hang.
const HOOK_TIMEOUT: u64 = 10;

pub(crate) fn is_fapony_blocker(cmd: &str) -> bool {
    cmd.contains("fapony") && (cmd.contains("hook-stop") || cmd.contains("hook-session-start"))
}

pub(crate) fn hooks_json(
    c: &Ctx,
    path: &Path,
    client: &str,
    want: &[(&str, Option<&str>, &str)],
) -> Result<(), String> {
    let mut root: Value = match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v @ Value::Object(_)) => v,
            _ => {
                out!(
                    c,
                    "  ! {} is not a JSON object — left alone, hooks not installed",
                    path.display()
                );
                return Ok(());
            }
        },
        Err(_) => json!({}),
    };
    let Some(hooks) = root
        .as_object_mut()
        .map(|o| o.entry("hooks").or_insert_with(|| json!({})))
        .and_then(Value::as_object_mut)
    else {
        out!(
            c,
            "  ! {} has a non-object \"hooks\" — left alone",
            path.display()
        );
        return Ok(());
    };
    let mut changed = vec![];
    for (event, matcher, sub) in want {
        let cmd = c.command(sub, client);
        let list = hooks.entry(*event).or_insert_with(|| json!([]));
        let Some(groups) = list.as_array_mut() else {
            continue;
        };
        if let Some(label) = adopt_hook(groups, *matcher, event, sub, client, &cmd) {
            changed.push(label);
        }
    }
    // fapony's Stop looks for rows in .fapony/ — next to fael it blocks every turn
    let mut fapony = 0;
    for groups in hooks.values_mut().filter_map(Value::as_array_mut) {
        for g in groups.iter_mut() {
            if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = hs.len();
                if c.replace {
                    hs.retain(|h| !h["command"].as_str().is_some_and(is_fapony_blocker));
                    fapony += before - hs.len();
                } else {
                    fapony += hs
                        .iter()
                        .filter(|h| h["command"].as_str().is_some_and(is_fapony_blocker))
                        .count();
                }
            }
        }
        if c.replace {
            groups.retain(|g| g["hooks"].as_array().is_none_or(|h| !h.is_empty()));
        }
    }
    if fapony > 0 && c.replace {
        changed.push(format!("removed {fapony} fapony hook(s)"));
    } else if fapony > 0 {
        out!(
            c,
            "  ! fapony's Stop/session-start hooks are still in {} — in a repo with .fael/ both block; \
             rerun with --replace-fapony once that repo's log is imported",
            path.display()
        );
    }
    if changed.is_empty() {
        out!(c, "  hooks already set in {}", path.display());
        return Ok(());
    }
    let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())? + "\n";
    c.write(path, &body)?;
    c.say(&format!("hooks {}", changed.join(", ")), path);
    Ok(())
}

/// Point this event's fael hook at `cmd`, wherever it sits in `groups`:
/// repoint the first entry matching the bare subcommand (a pre-`--client`
/// install) or this client's suffix, drop any duplicates, or append a fresh
/// group when none exists. Returns the `changed` label when it did anything —
/// the old one-entry `find` left a duplicate firing beside the new hook.
fn adopt_hook(
    groups: &mut Vec<Value>,
    matcher: Option<&str>,
    event: &str,
    sub: &str,
    client: &str,
    cmd: &str,
) -> Option<String> {
    let suffix = format!(" hook {sub} --client {client}");
    let bare = format!(" hook {sub}");
    let ours = |h: &Value| {
        h["command"]
            .as_str()
            .is_some_and(|s| s.contains("fael") && (s.ends_with(&suffix) || s.ends_with(&bare)))
    };
    let mut hits: Vec<(usize, usize)> = vec![];
    for (gi, g) in groups.iter().enumerate() {
        if let Some(hs) = g.get("hooks").and_then(Value::as_array) {
            for (hi, _) in hs.iter().enumerate().filter(|(_, h)| ours(h)) {
                hits.push((gi, hi));
            }
        }
    }
    let Some((gi, hi)) = hits.first().copied() else {
        let mut g = Map::new();
        if let Some(m) = matcher {
            g.insert("matcher".into(), json!(m));
        }
        g.insert(
            "hooks".into(),
            json!([{"type": "command", "command": cmd, "timeout": HOOK_TIMEOUT}]),
        );
        groups.push(Value::Object(g));
        return Some(match matcher {
            Some(m) => format!("{event}({m})"),
            None => event.to_string(),
        });
    };
    let changed = {
        let h = &mut groups[gi]["hooks"][hi];
        // a timeout the user set is theirs; only a missing one is added
        let timed = h.get("timeout").is_none().then(|| {
            h["timeout"] = json!(HOOK_TIMEOUT);
            format!("{event} (timeout)")
        });
        if h["command"] == json!(cmd) {
            (hits.len() > 1)
                .then(|| format!("{event} ({} duplicate hook)", hits.len() - 1))
                .or(timed)
        } else {
            h["command"] = json!(cmd);
            Some(format!("{event} (repointed)"))
        }
    };
    for &(gi, hi) in hits[1..].iter().rev() {
        if let Some(hs) = groups[gi]["hooks"].as_array_mut() {
            hs.remove(hi);
        }
    }
    groups.retain(|g| g["hooks"].as_array().is_none_or(|h| !h.is_empty()));
    changed
}
