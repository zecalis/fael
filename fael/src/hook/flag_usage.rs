//! The `fael stats` line for calls per command and flag (PLAN cli-tiers chunk
//! measure): the most-called few, agent vs human, and the day the counts start.
//! The numbers are `fael-core::stats::by_flag`.

use crate::core::stats::FlagUsage;

const TOP: usize = 8;

/// Nothing counted = no line.
pub(super) fn lines(u: &FlagUsage) -> Vec<String> {
    if u.by_flag.is_empty() {
        return vec![];
    }
    let top: Vec<String> = u
        .by_flag
        .iter()
        .take(TOP)
        .map(|f| {
            let name = match &f.flag {
                Some(n) => format!("{} --{n}", f.cmd),
                None => f.cmd.clone(),
            };
            format!("{name} ×{} ({} agent, {} human)", f.calls, f.agent, f.human)
        })
        .collect();
    vec![format!(
        "  calls per command and flag (since {}): {}",
        u.since,
        top.join(", ")
    )]
}
