//! Plans and chunks (SPEC-fael-board): the markdown reader, the SQLite store, and the
//! chunk state table. A state changes only by a command or by live git — never guessed.

mod export;
mod import;
pub mod md;
mod next;
mod schema;
mod store;

pub use export::export;
pub use next::{Next, next};
pub use store::{Import, PlanRow, Report, Store};

/// A chunk's one stored state (SPEC §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Draft,
    Open,
    Running,
    Waiting,
    Review,
    Done,
    Replaced,
    Dropped,
    Parked,
}

const ALL: [State; 9] = [
    State::Draft,
    State::Open,
    State::Running,
    State::Waiting,
    State::Review,
    State::Done,
    State::Replaced,
    State::Dropped,
    State::Parked,
];

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Draft => "draft",
            State::Open => "open",
            State::Running => "running",
            State::Waiting => "waiting",
            State::Review => "review",
            State::Done => "done",
            State::Replaced => "replaced",
            State::Dropped => "dropped",
            State::Parked => "parked",
        }
    }

    pub fn parse(s: &str) -> Option<State> {
        ALL.into_iter().find(|st| st.as_str() == s)
    }

    /// Meets an `after` that names it.
    pub fn closed(self) -> bool {
        matches!(self, State::Done | State::Dropped)
    }

    /// No transition leaves it.
    pub fn terminal(self) -> bool {
        matches!(self, State::Done | State::Dropped | State::Replaced)
    }

    /// The command that sets this state — what a rejected transition points at.
    fn command(self) -> &'static str {
        match self {
            State::Draft => "fael chunk add (no brief)",
            State::Open => "fael chunk add (with a brief) · fael chunk answer · fael chunk unpark",
            State::Running => "fael chunk start",
            State::Waiting => "fael chunk wait --on owner|data|<chunk>",
            State::Review => "fael chunk done --pr N",
            State::Done => "fael chunk done (no PR) · the PR merging",
            State::Replaced => "fael chunk apply (merge · split)",
            State::Dropped => "fael chunk drop",
            State::Parked => "fael chunk park",
        }
    }
}

/// SPEC §1: may a chunk in `from` move to `to`? The reject names the state's command
/// and the states it can be reached from.
pub fn check(from: State, to: State) -> Result<(), String> {
    if allowed(from, to) {
        return Ok(());
    }
    let froms: Vec<&str> = ALL
        .into_iter()
        .filter(|&f| f != to && allowed(f, to))
        .map(State::as_str)
        .collect();
    Err(format!(
        "rejected: a {} chunk cannot become {} — {} sets it, from {}",
        from.as_str(),
        to.as_str(),
        to.command(),
        if froms.is_empty() {
            "nothing".to_string()
        } else {
            froms.join(" | ")
        }
    ))
}

fn allowed(from: State, to: State) -> bool {
    use State::*;
    !from.terminal()
        && match to {
            Open => matches!(from, Draft | Waiting | Parked),
            Running => from == Open,
            Waiting => from == Running,
            Review => from == Running,
            Done => matches!(from, Running | Review),
            Replaced | Dropped => true,
            Parked => matches!(from, Draft | Open | Waiting),
            Draft => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_round_trip() {
        for s in ALL {
            assert_eq!(State::parse(s.as_str()), Some(s));
        }
        assert_eq!(State::parse("blocked"), None);
    }

    #[test]
    fn the_spec_table() {
        use State::*;
        for (from, to) in [
            (Draft, Open),
            (Open, Running),
            (Running, Waiting),
            (Waiting, Open),
            (Running, Review),
            (Running, Done),
            (Review, Done),
            (Open, Replaced),
            (Review, Dropped),
            (Open, Parked),
            (Parked, Open),
        ] {
            assert!(check(from, to).is_ok(), "{from:?} → {to:?}");
        }
        for (from, to) in [
            (Draft, Running),
            (Open, Done),
            (Waiting, Running),
            (Done, Open),
            (Replaced, Dropped),
            (Running, Parked),
        ] {
            assert!(check(from, to).is_err(), "{from:?} → {to:?}");
        }
        let e = check(Open, Done).unwrap_err();
        assert!(
            e.contains("fael chunk done") && e.contains("from running | review"),
            "{e}"
        );
    }
}
