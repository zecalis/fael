//! What landed on the default branch after the brief was written: printed above the TLDR so
//! the agent reads it before acting on the brief. Facts only — whether the brief still holds
//! is the agent's call, from the code (the chunk rules: brief is intent, code is truth).

use super::Brief;
use std::fmt::Write;

/// Silent when nothing landed, or the cli layer could not say (no scope, no baseline, no git).
pub(super) fn landed(o: &mut String, c: &Brief) {
    if c.landed.is_empty() {
        return;
    }
    let day = c
        .started
        .as_deref()
        .and_then(|s| s.get(..10))
        .unwrap_or("?");
    let n = c.landed.len() + c.landed_more;
    let _ = writeln!(
        o,
        "\nlanded since this brief was written ({day}): {n} commit(s) on the default branch touch its scope — the brief may describe code that has changed:"
    );
    for l in &c.landed {
        let _ = writeln!(o, "- {l}");
    }
    if c.landed_more > 0 {
        let _ = writeln!(o, "({} more not shown)", c.landed_more);
    }
    let _ = writeln!(
        o,
        "→ the brief says what was wrong then, not how to fix it now: read the code and decide. Already fixed → `fael chunk done {0} \"<the commit that fixed it>\" --left none --no-change`; the problem changed → fix what is wrong now and say what changed in the hand-off; changed past the brief's Goal → `fael chunk wait {0} --on owner \"<what changed>\"`",
        c.uid
    );
}
