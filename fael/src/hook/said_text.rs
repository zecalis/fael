//! The `fael stats` text for yield per line kind: acted-on rate and tokens per
//! kind, then the same per hook event (`edit` against `shell-edit`) — what the
//! noise work reads. Format only; every number comes from `fael-core::stats`
//! (`said.rs`). Tokens exist only for usage written since `t` did, so a window
//! reaching back shows them low or not at all.

use crate::core::{self, stats::KindYield};
use std::collections::BTreeMap;

type Said = BTreeMap<String, KindYield>;

/// ` ~420 tok`, nothing when there are none.
fn tok(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!(" ~{n} tok")
    }
}

/// `row 3/10 (30%) ~420 tok` per kind, in `SayKind::buckets` order, the kinds
/// never said left out; nothing said = no line.
pub(super) fn said_line(said: &Said) -> Option<String> {
    let parts: Vec<String> = core::SayKind::buckets()
        .chain(core::stats::ASK_SPLIT)
        .filter_map(|k| said.get(k).map(|y| (k, y)))
        .filter(|(_, y)| y.said > 0)
        .map(|(k, y)| {
            let rate = y.earned * 100 / y.said;
            format!("{k} {}/{} ({rate}%){}", y.earned, y.said, tok(y.tokens))
        })
        .collect();
    (!parts.is_empty()).then(|| format!("  acted on after said: {}", parts.join(" · ")))
}

/// The two events a push puts rows and hints in front of the agent at (rows
/// push only there since #339): the split the noise work reads. Every event
/// stays in `--json` (`said.<kind>.by_event`).
const PUSH_EVENTS: [&str; 2] = ["edit", "shell-edit"];

/// `row [edit 2/8 ~300 tok, shell-edit 1/2]` per kind: earned/said at the
/// `edit` and `shell-edit` events, those a kind was said at. No such entry =
/// no line.
pub(super) fn said_events_line(said: &Said) -> Option<String> {
    let parts: Vec<String> = core::SayKind::buckets()
        .filter_map(|k| said.get(k).map(|y| (k, y)))
        .filter_map(|(k, y)| {
            let evs: Vec<String> = PUSH_EVENTS
                .iter()
                .filter_map(|e| y.by_event.get(*e).map(|v| (e, v)))
                .map(|(e, v)| format!("{e} {}/{}{}", v.earned, v.said, tok(v.tokens)))
                .collect();
            (!evs.is_empty()).then(|| format!("{k} [{}]", evs.join(", ")))
        })
        .collect();
    (!parts.is_empty()).then(|| format!("  said by event: {}", parts.join(" · ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::stats::EventYield;

    #[test]
    fn tokens_and_the_edit_split_print_compactly() {
        let ev = |said, earned, tokens| EventYield {
            said,
            earned,
            tokens,
        };
        let mut s = Said::new();
        assert_eq!((said_line(&s), said_events_line(&s)), (None, None));
        s.insert(
            "row".into(),
            KindYield {
                said: 10,
                earned: 3,
                tokens: 320,
                by_event: [
                    ("edit".to_string(), ev(8, 2, 300)),
                    ("shell-edit".to_string(), ev(2, 1, 20)),
                ]
                .into(),
                ..KindYield::default()
            },
        );
        // other events stay out of the text
        s.insert(
            "bodies".into(),
            KindYield {
                said: 1,
                by_event: [("read".to_string(), ev(1, 1, 0))].into(),
                ..KindYield::default()
            },
        );
        s.insert(
            "ask".into(),
            KindYield {
                said: 4,
                earned: 1,
                by_event: [("edit".to_string(), ev(4, 1, 0))].into(),
                ..KindYield::default()
            },
        );
        assert_eq!(
            said_line(&s).as_deref(),
            Some(
                "  acted on after said: row 3/10 (30%) ~320 tok · ask 1/4 (25%) · bodies 0/1 (0%)"
            )
        );
        assert_eq!(
            said_events_line(&s).as_deref(),
            Some(
                "  said by event: row [edit 2/8 ~300 tok, shell-edit 1/2 ~20 tok] · ask [edit 1/4]"
            )
        );
    }
}
