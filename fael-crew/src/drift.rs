//! Brief drift of a running chunk (plan fael-chunk-drift): the query is core's, so the
//! hook and `chunk` output agree; this is its door on a `Store`.

use super::store::Store;
pub use fael_core::work::Drift;

impl Store {
    /// What the owner edited (brief, scope, title) since `uid`'s latest start.
    pub fn drift(&self, uid: &str) -> Option<Drift> {
        fael_core::work::drift_in(&self.conn, uid)
    }
}

#[cfg(test)]
mod tests {
    use super::super::chunk::tests::{NOW, add, start};
    use super::super::{Fields, Store};

    #[test]
    fn a_brief_edit_after_start_drifts_one_before_does_not() {
        let mut s = Store::open_in_memory().unwrap();
        let u = add(&mut s, "a", Some("a"));
        let edit = |s: &mut Store, b: &str| {
            let f = Fields {
                brief: Some(b.into()),
                ..Default::default()
            };
            s.edit(&u, &f, NOW).unwrap();
        };
        edit(&mut s, "before");
        start(&mut s, &u, "/w").unwrap();
        assert_eq!(s.drift(&u), None, "edited before the start");
        edit(&mut s, "after");
        let d = s.drift(&u).unwrap();
        assert_eq!(d.fields, ["brief"]);
        start(&mut s, &u, "/w").unwrap();
        assert_eq!(s.drift(&u), None, "a new start resets it");
    }
}
