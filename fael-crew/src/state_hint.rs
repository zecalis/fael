//! What a refused move to done/review says past the state table (`lib.rs::check`). The table
//! ends with "try <the command you just ran> to set it" — circular for a chunk that never
//! started, is parked, waits or is history. This names the one step that gets it moving
//! (or says nothing does), from the state the chunk is in.

use super::State;

/// The tail appended to the table's reject when `from` cannot become `to`; None = the
/// table's own text already names the move (any `to` but done/review, or a state that
/// can reach it).
pub(super) fn way_on(from: State, to: State, uid: &str) -> Option<String> {
    if !matches!(to, State::Done | State::Review) {
        return None;
    }
    match from {
        State::Open => Some(format!(" — start first: `fael chunk start {uid}`")),
        State::Draft => Some(format!(
            " — it is draft (added with no brief): the owner or planner adds a brief (`fael chunk edit {uid} --brief \"<why + done>\"`), then `fael chunk start {uid}`"
        )),
        State::Waiting => Some(format!(
            " — it is waiting: the owner's answer unblocks it (`fael chunk answer {uid} \"<text>\"` or the board; the question: `fael chunk show {uid}`), then `fael chunk start {uid}`"
        )),
        State::Parked => Some(format!(
            " — it is parked: `fael chunk unpark {uid}`, then `fael chunk start {uid}`"
        )),
        State::Dropped | State::Replaced => Some(format!(
            " — it is {}, history that nothing moves again: try `fael chunk add \"<title>\" --plan <plan>` for new work (`fael chunk show {uid}` names its plan)",
            from.as_str()
        )),
        // `--out` skips the amend a plain second done takes
        State::Done => Some(format!(
            " — it is already done: `fael chunk done {uid} \"<new hand-off>\"` amends its hand-off (add --pr N[,M...] to correct the PR); --out belongs to the first done"
        )),
        State::Running | State::Review => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_that_cannot_reach_done_names_its_own_way_on() {
        let u = "01AAAAAAAA0000000000000000";
        for (from, needle) in [
            (
                State::Open,
                "start first: `fael chunk start 01AAAAAAAA0000000000000000`",
            ),
            (
                State::Draft,
                "`fael chunk edit 01AAAAAAAA0000000000000000 --brief",
            ),
            (
                State::Waiting,
                "`fael chunk answer 01AAAAAAAA0000000000000000 \"<text>\"`",
            ),
            (
                State::Parked,
                "`fael chunk unpark 01AAAAAAAA0000000000000000`, then `fael chunk start",
            ),
            (State::Dropped, "it is dropped, history"),
            (State::Replaced, "it is replaced, history"),
            (
                State::Done,
                "`fael chunk done 01AAAAAAAA0000000000000000 \"<new hand-off>\"` amends",
            ),
        ] {
            for to in [State::Done, State::Review] {
                let t = way_on(from, to, u).unwrap_or_default();
                assert!(t.contains(needle), "{from:?}→{to:?}: {t}");
            }
        }
    }

    #[test]
    fn a_state_that_can_reach_it_or_another_target_adds_nothing() {
        let u = "01AAAAAAAA0000000000000000";
        assert_eq!(way_on(State::Running, State::Done, u), None);
        assert_eq!(way_on(State::Review, State::Done, u), None);
        assert_eq!(way_on(State::Open, State::Waiting, u), None);
        assert_eq!(way_on(State::Parked, State::Running, u), None);
    }
}
