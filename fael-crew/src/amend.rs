//! `chunk done` on a done chunk amends its hand-off (plan fael-chunk-drift): one new agent
//! event, nothing else changes. `handoff::after` and `by_file` take the newest agent done
//! event, so every later brief prints the amended text.

use super::State;
use super::chunk::{row, set, tx};
use super::store::{Store, err};

impl Store {
    /// `Ok(true)` when `uid` was done and its hand-off is now `handoff`; `Ok(false)` when it
    /// is in another state (the caller runs the normal done, which rejects a dropped chunk).
    pub fn amend(&mut self, uid: &str, handoff: &str, now: &str) -> Result<bool, String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        if r.state != State::Done {
            return Ok(false);
        }
        set(&tx, &r, State::Done, "agent", Some(handoff), now)?;
        tx.commit().map_err(err)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::chunk::tests::{NOW, add, here, start};
    use super::super::{Fields, Owner, Start, Store};

    #[test]
    fn a_second_done_replaces_the_handoff_the_next_brief_prints() {
        let mut s = Store::open_in_memory().unwrap();
        let a = add(&mut s, "a", Some("b"));
        let f = Fields {
            title: Some("next".into()),
            brief: Some("b".into()),
            ..Fields::default()
        };
        let b = s.add("inbox", &f, std::slice::from_ref(&a), NOW).unwrap();
        start(&mut s, &a, "/a").unwrap();
        s.done(&a, "first words", Some(1), None, &here("/a"))
            .unwrap();
        assert!(s.amend(&a, "amended words", NOW).unwrap());
        let st = s.start(&b, &Start::default(), &here("/b")).unwrap();
        let after = &st.chunks[0].after;
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].1, "amended words");
    }

    #[test]
    fn only_a_done_chunk_is_amended() {
        let mut s = Store::open_in_memory().unwrap();
        let a = add(&mut s, "a", Some("b"));
        assert!(!s.amend(&a, "x", NOW).unwrap(), "open");
        s.owner(&a, Owner::Drop, Some("no"), NOW).unwrap();
        assert!(!s.amend(&a, "x", NOW).unwrap(), "dropped");
        let err = s.done(&a, "x", Some(1), None, &here("/a")).unwrap_err();
        assert!(err.contains("rejected"), "{err}");
    }
}
