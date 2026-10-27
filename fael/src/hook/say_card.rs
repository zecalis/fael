//! The kind card: everything the noise contract knows about one push-line
//! `Kind`, in one exhaustive match (`policy`, no `_` arm). A new `Kind` does
//! not compile until it has a card here, and `say_contract` fails until it has
//! a fixture there. The card says:
//!
//! - `said` — the `SayKind` name its `said` entries carry in `usage.jsonl`;
//!   `fael-core` stats tallies by that same enum, so a kind said is a kind
//!   counted (issue 01M5ZH6V);
//! - `once` and `command` — how it is said once, and the command its text must
//!   offer, or the line is dropped;
//! - `at` — the hook events it may be said at, read off where its `Line` is
//!   built; an `Outbox` opened for another event drops it;
//! - `cut` — whether, and in what order, a push over budget drops it;
//! - `stash` — whether it is a stashed line the push takes off disk when said.

use super::say::Kind;
use super::search::SHELL_EDIT;
use crate::core::SayKind;

/// The hook event an `Outbox` is opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    /// `session-start`: the brief and its warnings.
    SessionStart,
    /// The `edit` push (`push.rs`).
    Edit,
    /// A shell call that wrote a file, pushed as an edit (`search.rs`).
    ShellEdit,
    /// The `search` hook's `git commit` lines (`search.rs`, usage event `search`).
    Search,
    /// A `ReportFindings` call (`review.rs`).
    Review,
}

impl At {
    /// The usage label of a push at this event: `edit` or `shell-edit`.
    /// `None` for an event that is no push.
    pub(crate) fn push_label(self) -> Option<&'static str> {
        match self {
            At::Edit => Some("edit"),
            At::ShellEdit => Some(SHELL_EDIT),
            At::SessionStart | At::Search | At::Review => None,
        }
    }
}

/// The events of a push: rows, a retired note, a stashed line. No client
/// sends a read push (rows come at the edit), so a push is an edit or a shell edit.
const PUSH: &[At] = &[At::Edit, At::ShellEdit];
/// The events of an edit: what is only worth saying while the file is changing.
const EDIT: &[At] = &[At::Edit, At::ShellEdit];

/// How a kind is said only once.
#[derive(Debug, PartialEq)]
pub(crate) enum Once {
    /// Its keys are spent in the seen list; a line whose keys are all spent is dropped.
    Key,
    /// Its event fires once: a stashed notice is taken off disk when said.
    Event,
}

/// Whether a push over its token budget may drop a line of this kind, whole.
/// Lower ranks go first; of the lines of one kind, `First` drops the earliest
/// and `Last` the latest. A cut line keeps its keys for a later push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cut {
    Never,
    First(u8),
    Last(u8),
}

#[derive(Debug)]
pub(crate) struct Policy {
    pub(crate) said: SayKind,
    pub(crate) once: Once,
    /// The command the line must offer, found in its text, or it is dropped.
    /// Set here, never by the caller that writes the text.
    pub(crate) command: Option<&'static str>,
    pub(crate) at: &'static [At],
    pub(crate) cut: Cut,
    /// A stashed line: `say_within` reports it said so the caller clears the stash.
    pub(crate) stash: bool,
}

/// A card that is never cut and not stashed.
const fn card(
    said: SayKind,
    once: Once,
    command: Option<&'static str>,
    at: &'static [At],
) -> Policy {
    Policy {
        said,
        once,
        command,
        at,
        cut: Cut::Never,
        stash: false,
    }
}

/// The noise policy, in one place.
pub(crate) fn policy(k: &Kind) -> Policy {
    use Once::{Event, Key};
    match k {
        Kind::Row { .. } => card(SayKind::Row, Key, None, PUSH),
        Kind::Brief { .. } => card(SayKind::Brief, Key, None, &[At::SessionStart]),
        // edit only: at a shell edit it earned 6 of 95 (6%) against 60 of 107 (56%)
        // at an edit — decision product:push-noise-bar
        Kind::Ask { .. } => Policy {
            cut: Cut::Last(2),
            ..card(SayKind::Ask, Key, Some("fael close"), &[At::Edit])
        },
        Kind::Cited { .. } => Policy {
            cut: Cut::First(3),
            ..card(SayKind::Cited, Key, Some("fael close"), &[At::Search])
        },
        Kind::Finding { .. } => card(SayKind::Finding, Key, Some("fael add issue"), &[At::Review]),
        Kind::Fixed => Policy {
            cut: Cut::First(1),
            stash: true,
            ..card(SayKind::Fixed, Key, Some("fael close"), PUSH)
        },
        Kind::Overlap { .. } => card(SayKind::Overlap, Key, Some("fael chunk note"), EDIT),
        Kind::Checkpoint { .. } => card(
            SayKind::Checkpoint,
            Key,
            Some("fael chunk note"),
            &[At::Search],
        ),
        Kind::BriefDrift { .. } => card(SayKind::BriefDrift, Key, Some("fael chunk show"), EDIT),
        Kind::Retired { .. } => card(SayKind::Retired, Key, Some("fael add decision"), PUSH),
        // a session-start warning, or a stashed risk at a push
        Kind::Notice => Policy {
            cut: Cut::First(0),
            stash: true,
            ..card(
                SayKind::Notice,
                Event,
                None,
                &[At::SessionStart, At::Edit, At::ShellEdit],
            )
        },
    }
}

/// The index of the one line an over-budget push drops next, given each
/// line's `cut` in order; `None` when only lines that are never cut remain.
pub(super) fn next_cut(cuts: impl Iterator<Item = Cut>) -> Option<usize> {
    cuts.enumerate()
        .filter_map(|(i, cut)| match cut {
            Cut::Never => None,
            Cut::First(rank) => Some((rank, i, i)),
            // the latest of its kind: a bigger index sorts first
            Cut::Last(rank) => Some((rank, usize::MAX - i, i)),
        })
        .min()
        .map(|(_, _, i)| i)
}
