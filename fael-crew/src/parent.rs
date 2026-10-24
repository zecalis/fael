//! `plan.parent` (issue 01M51KW0): a plan names one umbrella plan of the same app, one level
//! deep — a fact the board shows, never a rule on the chunks (an umbrella may hold none).

use super::store::err;
use rusqlite::{Transaction, params};

/// Point plan `pid` (`app`/`name`) at `parent`; `""` clears it. Rejects a parent that is the
/// plan itself, is missing, is a child already, or a plan that has children of its own.
pub(super) fn set(
    tx: &Transaction,
    pid: i64,
    app: &str,
    name: &str,
    parent: &str,
) -> Result<(), String> {
    if parent.is_empty() {
        tx.execute("UPDATE plan SET parent = NULL WHERE id = ?1", [pid])
            .map_err(err)?;
        return Ok(());
    }
    if parent == name {
        return Err(format!(
            "rejected: plan {name} cannot be its own parent; try `fael plan list` to pick another"
        ));
    }
    let up: Option<Option<String>> = tx
        .query_row(
            "SELECT parent FROM plan WHERE app = ?1 AND name = ?2",
            params![app, parent],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })
        .map_err(err)?;
    match up {
        None => Err(format!(
            "rejected: no plan '{parent}' to be the parent of {name} — a parent is a plan of the same app; try `fael plan list` to pick one"
        )),
        Some(Some(up)) => Err(format!(
            "rejected: plan {parent} is itself a child of {up} — one level only; try `fael plan list` to pick a top-level plan"
        )),
        Some(None) => {
            let kids: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM plan WHERE app = ?1 AND parent = ?2",
                    params![app, name],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if kids > 0 {
                return Err(format!(
                    "rejected: plan {name} has {kids} child plan(s) — one level only; try `fael plan drop <child>` for its children first"
                ));
            }
            tx.execute(
                "UPDATE plan SET parent = ?1 WHERE id = ?2",
                params![parent, pid],
            )
            .map_err(err)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Bill, Store, board::Board, export_json};
    use serde_json::json;

    const NOW: &str = "2026-10-16T00:00:00Z";

    fn store() -> Store {
        let mut s = Store::open_in_memory().unwrap();
        s.new_plan("", "umbrella", "U", "dev", None, NOW).unwrap();
        s.new_plan("", "page-a", "A", "dev", Some("umbrella"), NOW)
            .unwrap();
        s
    }

    fn plan(s: &Store, name: &str) -> i64 {
        s.plans()
            .unwrap()
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .id
    }

    fn bill(s: &mut Store, name: &str, edit: serde_json::Value) -> Result<Vec<String>, String> {
        let mut b = export_json(s, plan(s, name)).unwrap();
        b["chunks"] = json!([]);
        b["sections"] = json!([]);
        b["parent"] = edit;
        s.bill(serde_json::from_value::<Bill>(b).unwrap(), NOW, false)
            .map(|(l, _)| l)
    }

    #[test]
    fn new_with_a_parent_exports_and_shows_on_the_board() {
        let s = store();
        assert_eq!(
            export_json(&s, plan(&s, "page-a")).unwrap()["parent"],
            "umbrella"
        );
        assert!(export_json(&s, plan(&s, "umbrella")).unwrap()["parent"].is_null());
        let b: Board = s.board("2026-10-16", "").unwrap();
        let parents: Vec<_> = b
            .plans
            .iter()
            .map(|p| (p.name.as_str(), p.parent.as_deref()))
            .collect();
        assert!(
            parents.contains(&("page-a", Some("umbrella"))),
            "{parents:?}"
        );
        assert!(parents.contains(&("umbrella", None)), "{parents:?}");
        // absent = top level: the key is not written at all
        let top = b.plans.iter().find(|p| p.name == "umbrella").unwrap();
        assert!(serde_json::to_value(top).unwrap().get("parent").is_none());
    }

    #[test]
    fn one_level_missing_and_self_are_refused() {
        let mut s = store();
        for (name, parent, want) in [
            ("page-b", "nope", "no plan 'nope'"),
            ("page-b", "page-b", "its own parent"),
            ("page-b", "page-a", "itself a child of umbrella"),
        ] {
            let e = s
                .new_plan("", name, "B", "dev", Some(parent), NOW)
                .unwrap_err();
            assert!(e.contains(want), "{e}");
        }
        // a refused new leaves no plan behind
        assert_eq!(s.plans().unwrap().len(), 2);
        // a plan with children cannot become a child
        s.new_plan("", "top", "T", "dev", None, NOW).unwrap();
        let e = bill(&mut s, "umbrella", json!("top")).unwrap_err();
        assert!(e.contains("child plan"), "{e}");
    }

    #[test]
    fn done_refuses_a_live_child_and_undone_brings_the_parent_back() {
        let mut s = store();
        let up = plan(&s, "umbrella");
        // an active child blocks the parent's done, naming it
        let e = s.plan_state(up, "done", NOW).unwrap_err();
        assert!(e.contains("child plan(s) not done (page-a)"), "{e}");
        // a parked child is still not done — still refused
        let kid = plan(&s, "page-a");
        s.plan_state(kid, "parked", NOW).unwrap();
        let e = s.plan_state(up, "done", NOW).unwrap_err();
        assert!(e.contains("page-a"), "{e}");
        // child done first, then the parent follows
        s.plan_state(kid, "done", NOW).unwrap();
        assert_eq!(s.plan_state(up, "done", NOW).unwrap(), "active");
        // undone brings the done parent back; the child stays done
        assert_eq!(s.plan_state(up, "active", NOW).unwrap(), "done");
        let states: Vec<_> = s
            .plans()
            .unwrap()
            .into_iter()
            .map(|p| (p.name, p.state))
            .collect();
        assert!(states.contains(&("umbrella".to_string(), "active".to_string())));
        assert!(states.contains(&("page-a".to_string(), "done".to_string())));
        // undone on an active plan is a reject, naming its state
        let e = s.plan_state(up, "active", NOW).unwrap_err();
        assert!(e.contains("umbrella is active"), "{e}");
    }

    #[test]
    fn a_bill_sets_moves_and_clears_it() {
        let mut s = store();
        s.new_plan("", "other", "O", "dev", None, NOW).unwrap();
        s.new_plan("", "page-c", "C", "dev", None, NOW).unwrap();
        let l = bill(&mut s, "page-c", json!("other")).unwrap();
        assert_eq!(l, ["plan page-c: edited parent"]);
        assert_eq!(
            export_json(&s, plan(&s, "page-c")).unwrap()["parent"],
            "other"
        );
        // sent back unchanged: no write
        assert!(bill(&mut s, "page-c", json!("other")).unwrap().is_empty());
        bill(&mut s, "page-c", json!("")).unwrap();
        assert!(export_json(&s, plan(&s, "page-c")).unwrap()["parent"].is_null());
        // the feed carries the column whole
        let f = super::super::feed(&s, 0).unwrap();
        assert!(
            f["plans"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["parent"] == "umbrella")
        );
    }
}
