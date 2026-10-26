//! The prompt hook: says nothing. The key pointer it used to say earned
//! 497 of 2214 (22%, decision 01M5528X) and was cut, and the per-turn mark it
//! kept for the gone-check ask went with that ask.

use super::protocol::{Event, Reply};

pub(crate) fn prompt(_e: &Event) -> Reply {
    Reply::default()
}
