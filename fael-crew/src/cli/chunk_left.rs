//! `chunk done --left none|<id|uid>,…` (issue 01M5FMHQX): what the chunk leaves undone is a
//! row the next session finds, not prose in a PR body. a `left:` line in the hand-off
//! says the same and counts as the flag.

use fael_crew::Store;

const HOW: &str = "file the leftover first: `fael chunk add \"<title>\" --plan <plan> --brief \"…\"` or `fael add issue \"<text>\" --files <f>`; `--left none` when nothing is left";

/// The value of the hand-off's `left:` line.
fn line(handoff: &str) -> Option<String> {
    handoff
        .lines()
        .find_map(|l| l.trim().strip_prefix("left:"))
        .map(|v| v.trim().to_string())
}

/// The ids/uids a stored hand-off leaves undone; `none`, no line or no hand-off = empty.
pub(crate) fn listed(handoff: &str) -> Vec<String> {
    line(handoff)
        .filter(|l| l != "none")
        .map(|l| l.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_default()
}

/// The hand-off to store: `handoff` with the validated statement on a `left:` line.
pub(crate) fn resolve(
    flag: Option<String>,
    handoff: &str,
    s: &Store,
    log: &fael_core::Log,
) -> Result<String, String> {
    let line = line(handoff);
    let (list, text) = match (flag, line) {
        (Some(_), Some(_)) => {
            return Err("rejected: --left and a `left:` line both given — once".into());
        }
        (Some(f), None) => (f.clone(), format!("{}\nleft: {f}", handoff.trim_end())),
        (None, Some(l)) => (l, handoff.to_string()),
        (None, None) => {
            return Err(format!(
                "rejected: chunk done needs --left none or --left <id|uid>,… — {HOW}"
            ));
        }
    };
    let toks: Vec<&str> = list.split(',').map(str::trim).collect();
    if toks == ["none"] {
        return Ok(text);
    }
    let shut = fael_core::closed(log);
    let bad: Vec<&str> = toks
        .iter()
        .copied()
        .filter(|t| {
            !(s.is_chunk(t)
                || matches!(fael_core::ref_state(log, t),
                    fael_core::Ref::One(r) if !shut.contains(r.id.as_str())))
        })
        .collect();
    if !bad.is_empty() {
        return Err(format!(
            "rejected: --left {} is no open ledger row or chunk of this db — {HOW}",
            bad.join(", ")
        ));
    }
    Ok(text)
}
