//! The headline of rows a push matched but did not render, so `(1 of 8)` also
//! says what the other 7 are (issue fael:01M62VMF). At most `SHOWN` of them.

use crate::core;

const SHOWN: usize = 3;

/// ` · also: <id> "<title>", …` for the first rows past `rendered`, then the
/// rows `select` cut; empty when none is left.
pub(super) fn also(log: &core::Log, sel: &core::Selection, rendered: usize) -> String {
    let abbrev = core::abbrev(log);
    let rows = sel
        .shown
        .iter()
        .skip(rendered)
        .copied()
        .chain(sel.cut.iter().map(|c| c.0));
    let named: Vec<String> = rows
        .take(SHOWN)
        .map(|r| {
            let t = r.title.as_deref().unwrap_or(&r.text);
            let t: String = t.chars().take(60).collect();
            format!("{} \"{t}\"", abbrev.short(&r.id))
        })
        .collect();
    match named.is_empty() {
        true => String::new(),
        false => format!(" · also: {}", named.join(", ")),
    }
}
