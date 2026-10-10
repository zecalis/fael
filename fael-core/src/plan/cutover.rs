//! `fael plan cutover` (SPEC-fael-board §9): an md plan hands its chunk list to the db. The
//! caller re-imports first, so the db holds exactly the md's chunks (parity 0 diff); this
//! refuses what the db could not carry on alone, flips `truth`, and the md's chunk lines give
//! way to a banner. Goal, Scope, Done and the rest stay in the md as the truth.

use super::md;
use super::store::{Store, err};
use rusqlite::TransactionBehavior;

impl Store {
    /// Flip plan `id` to `truth = db` in one `BEGIN IMMEDIATE`. Rejected when it already is,
    /// or while a chunk is a draft (an unknown checkbox), held by a `(wip …)` marker (a run
    /// with no R), or waits on an `(after …)` naming no chunk. Returns its chunk count.
    pub fn cut_over(&mut self, id: i64) -> Result<usize, String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let (name, truth): (String, String) = tx
            .query_row("SELECT name, truth FROM plan WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(err)?;
        if truth == "db" {
            return Err(format!("rejected: {name} is already cut over"));
        }
        let bad: Vec<String> = tx
            .prepare(
                "SELECT COALESCE(c.label, c.title) || ': ' || CASE
                   WHEN c.state = 'draft' THEN 'unknown checkbox — use [ ], [x] or [~]'
                   WHEN c.state = 'running' THEN '(wip …) — finish or drop the claim first'
                   ELSE '(after ' || e.ref || ') names no chunk' END
                 FROM chunk c LEFT JOIN edge e
                   ON e.src = c.id AND e.kind = 'after' AND e.dst IS NULL
                 WHERE c.plan = ?1
                   AND (c.state IN ('draft','running') OR e.src IS NOT NULL)
                 ORDER BY c.seq",
            )
            .map_err(err)?
            .query_map([id], |r| r.get(0))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        if !bad.is_empty() {
            return Err(format!(
                "rejected: fix {name}'s md first, nothing changed:\n  {}",
                bad.join("\n  ")
            ));
        }
        tx.execute("UPDATE plan SET truth = 'db' WHERE id = ?1", [id])
            .map_err(err)?;
        let n: usize = tx
            .query_row("SELECT COUNT(*) FROM chunk WHERE plan = ?1", [id], |r| {
                r.get(0)
            })
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(n)
    }
}

/// The md with its TL;DR chunk lines — the checkboxes, or a banner and the mirror below
/// it — replaced by the banner and `ticks`, where the first one stood and at its indent.
/// `None` when the TL;DR holds neither.
pub fn mirror(text: &str, plan: &str, ticks: &[String]) -> Option<String> {
    let mut out = Vec::new();
    let (mut h2, mut done) = (0, false);
    for l in text.lines() {
        h2 += usize::from(md::is_h2(l));
        if h2 != 1 || (md::checkbox(l).is_none() && !l.trim_start().starts_with(BANNER)) {
            out.push(l.to_string());
            continue;
        }
        if !done {
            let indent = &l[..l.len() - l.trim_start().len()];
            out.push(format!(
                "{indent}{BANNER} — `fael board`, `fael plan export {plan}` (mirror: edits here change nothing)"
            ));
            out.extend(ticks.iter().map(|t| format!("{indent}{t}")));
            done = true;
        }
    }
    done.then(|| out.join("\n") + if text.ends_with('\n') { "\n" } else { "" })
}

const BANNER: &str = "- chunks live in fael";

impl Store {
    /// Plan `id`'s chunks as checkbox lines for [`mirror`], state named when it is neither
    /// open nor closed.
    pub fn ticks(&self, id: i64) -> Result<Vec<String>, String> {
        self.conn
            .prepare("SELECT title, state FROM chunk WHERE plan = ?1 ORDER BY seq")
            .map_err(err)?
            .query_map([id], |r| {
                let (title, state): (String, String) = (r.get(0)?, r.get(1)?);
                Ok(match state.as_str() {
                    "done" => format!("- [x] {title}"),
                    "dropped" | "replaced" => format!("- [~] {title}"),
                    "open" => format!("- [ ] {title}"),
                    s => format!("- [ ] {title} · **{s}**"),
                })
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Import;
    use super::*;

    const DOC: &str = "---\nkind: unit\n---\n\n# PLAN-t\n\n## TL;DR\n- **Progress:**\n  \
                       - [x] c1 — one\n  - [ ] c2 — two\n\n## 1. Goal\n- [ ] not a chunk\n";

    fn import(s: &mut Store, body: &str) -> i64 {
        let md = md::parse("PLAN-t.md", body).unwrap();
        let p = Import {
            app: String::new(),
            dir: "plan".into(),
            source: ".fapony/plan/PLAN-t.md".into(),
            md,
        };
        s.import_all(&[p], "t0").unwrap();
        s.plans().unwrap()[0].id
    }

    #[test]
    fn cut_over_flips_truth_once_and_a_reimport_keeps_the_chunks() {
        let mut s = Store::open_in_memory().unwrap();
        let id = import(&mut s, DOC);
        assert_eq!(s.cut_over(id), Ok(2));
        assert_eq!(s.plans().unwrap()[0].truth, "db");
        assert!(s.cut_over(id).unwrap_err().contains("already cut over"));
        // the mirrored md re-imported: plan fields refresh, the db's chunks win
        s.conn
            .execute("UPDATE chunk SET state = 'review' WHERE label = 'c2'", [])
            .unwrap();
        let md = mirror(DOC, "t", &s.ticks(id).unwrap()).unwrap();
        let banner = "  - chunks live in fael — `fael board`, `fael plan export t` (mirror: edits here change nothing)\n";
        let want = format!("{banner}  - [x] c1 — one\n  - [ ] c2 — two · **review**\n");
        assert_eq!(
            md,
            DOC.replace("  - [x] c1 — one\n  - [ ] c2 — two\n", &want)
        );
        import(&mut s, &md.replace("[x] c1", "[ ] c1"));
        assert_eq!(
            s.ticks(id).unwrap()[0],
            "- [x] c1 — one",
            "md edits change nothing"
        );
        // a second mirror replaces the first, never stacks
        assert_eq!(mirror(&md, "t", &s.ticks(id).unwrap()).unwrap(), md);
        assert_eq!(mirror("# P\n\n## TL;DR\n- x\n", "t", &[]), None);
    }

    #[test]
    fn cut_over_refuses_draft_wip_and_unresolved_after_and_changes_nothing() {
        let mut s = Store::open_in_memory().unwrap();
        let id = import(
            &mut s,
            "# PLAN-t\n\n## TL;DR\n- [?] c1 — odd\n- [ ] c2 (wip feat/a) — held\n\
             - [ ] c3 (after zz) — waits on nothing\n",
        );
        let e = s.cut_over(id).unwrap_err();
        for want in [
            "c1 — odd: unknown checkbox",
            "c2: (wip …)",
            "c3: (after zz) names no chunk",
        ] {
            assert!(e.contains(want), "{want} in {e}");
        }
        assert_eq!(s.plans().unwrap()[0].truth, "md");
    }
}
