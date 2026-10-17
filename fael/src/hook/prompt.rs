//! The prompt hook: every user prompt starts a turn, so the edit ask speaks
//! once per turn. It says nothing — the key pointer it used to say earned
//! 497 of 2214 (22%, decision 01M5528X) and was cut.

use super::protocol::{Event, Reply, ctx};
use super::state::new_turn;

/// No session = no turn to mark.
pub(crate) fn prompt(e: &Event) -> Reply {
    if let Some(c) = ctx(e).filter(|c| !c.session.is_empty()) {
        new_turn(&c.session, &c.repo.root);
    }
    Reply::default()
}
