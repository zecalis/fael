//! The shape faults `add` rejects before writing — re-exported from the
//! one text-shape module (`crate::shape`), so warnings, gates and stats read
//! the same predicates and never drift apart.

pub use crate::shape::add_gate;
pub(crate) use crate::shape::{topic_list, untitled};
