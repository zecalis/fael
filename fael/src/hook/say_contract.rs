//! The noise contract, looped over every `Kind` (PLAN-fael-say-gate): nothing
//! to say is silence, a key-once kind is not said twice in a session, a kind
//! that needs a command is dropped without one, and no session says it all.

use super::say::{Kind, Line, Outbox};
use super::say_card::{At, policy};
use std::path::PathBuf;

/// One fixture per kind. `slot` has no `_` arm, so a new `Kind` does not
/// compile until it gets a slot — and `every_kind_has_a_fixture` fails until
/// that slot has a fixture here.
pub(super) fn all() -> Vec<Line> {
    let line = |kind: Kind, text: &str| Line {
        kind,
        text: text.into(),
    };
    vec![
        line(
            Kind::Row {
                ids: vec!["01ROW".into()],
            },
            "fael mem for a.rs:\n- [01ROW] decision x\n",
        ),
        // session start: hand-offs and assigned rows, once per session
        line(
            Kind::Brief {
                keys: vec!["01BRIEF".into()],
            },
            "- [01BRIEF] note y #plan:x:handoff\n",
        ),
        line(
            Kind::Ask {
                ids: vec!["01ASK".into()],
            },
            "fael: done with one? fael close 01ASK \"<why>\"\n",
        ),
        line(
            Kind::Cited {
                ids: vec!["01CITED".into()],
            },
            "fael: 01CITED cited in a commit — done? fael close 01CITED \"<why>\"\n",
        ),
        line(
            Kind::Finding {
                file: "a.rs".into(),
                line: 7,
            },
            "fael: review finding on a.rs — file it: `fael add issue 'x' --files a.rs`\n",
        ),
        Line::notice("fael: a stashed line\n".into()),
        line(
            Kind::Fixed,
            "fael: this session said it fixed a bug — fael add issue \"x\" --key a:b then fael close --key a:b \"y\"\n",
        ),
        line(
            Kind::Overlap {
                owner: "01D".into(),
                file: "a.rs".into(),
            },
            "fael: a.rs — p/x (agent, open) · fael chunk note 01D \"<text>\" · yours? fael chunk status\n",
        ),
        line(
            Kind::Peer {
                file: "a.rs".into(),
            },
            "fael: a.rs also edited by claude on feat-x 2m ago — leave them a word: fael add note \"…\" --files a.rs\n",
        ),
        line(
            Kind::Inbox {
                id: "7".into(),
                uid: "01C".into(),
            },
            "fael: new note for chunk 01C: use v2 — reply: fael chunk note 01C \"<text>\" · full: fael chunk show 01C\n",
        ),
        line(
            Kind::Checkpoint {
                key: "~checkpoint:01C:0".into(),
            },
            "fael: checkpoint — after this commit: fael chunk note 01C \"<done; next>\" — the next chunk start resumes from it\n",
        ),
        line(
            Kind::Handoff,
            "fael: nothing filed this session — what must the next session know? fael add note \"…\" --files a.rs\n",
        ),
        line(
            Kind::BriefDrift {
                key: "~drift:01C@7".into(),
            },
            "fael: chunk 01C brief changed since you started (brief) — read it: fael chunk show 01C\n",
        ),
        line(
            Kind::Retired { id: "01N".into() },
            "fael: note 01N retired from the push after 5 sessions — a standing rule? re-file it: `fael add decision \"...\" --files a.rs --supersedes 01N`\n",
        ),
        line(
            Kind::ChunkBrief { uid: "01C".into() },
            "fael: this worktree runs chunk 01C \"t\" — its brief and hand-offs: fael chunk show 01C\n",
        ),
    ]
}

pub(super) fn slot(k: &Kind) -> usize {
    match k {
        Kind::Row { .. } => 0,
        Kind::Brief { .. } => 1,
        Kind::Ask { .. } => 2,
        Kind::Cited { .. } => 3,
        Kind::Finding { .. } => 4,
        Kind::Notice => 5,
        Kind::Fixed => 6,
        Kind::Overlap { .. } => 7,
        Kind::Peer { .. } => 8,
        Kind::Inbox { .. } => 9,
        Kind::Checkpoint { .. } => 10,
        Kind::Handoff => 11,
        Kind::BriefDrift { .. } => 12,
        Kind::Retired { .. } => 13,
        Kind::ChunkBrief { .. } => 14,
    }
}

/// A fresh seen list of its own.
pub(super) fn seen(name: &str) -> PathBuf {
    std::env::temp_dir()
        .join(format!("fael-say-{}", crate::core::ulid()))
        .join(name)
}

/// An event the kind may be said at.
fn at_of(l: &Line) -> At {
    policy(&l.kind).at[0]
}

fn said(file: Option<std::fs::File>, l: Line) -> Option<String> {
    let mut out = Outbox::open(at_of(&l), file);
    out.say(l);
    out.reply().context().map(String::from)
}

#[cfg(test)]
mod tests;
