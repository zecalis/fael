//! `fael chunk status <uid>`: both sides of a joint run at once (f6).
//! DB only — no git, no gh — so the CLI fills the live reads like board
//! conflicts do (`merge-tree`, CLI fills). No state change.

use super::lookup::{lookup, no_chunk};
use super::store::{Store, err};

/// One side of the joint run: what the CLI's git reads fill in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    pub uid: String,
    pub title: String,
    pub state: String,
    pub run: Option<String>,
    pub joint: Option<String>,
    pub partner: Option<String>,
    pub partners: Vec<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub worktree: Option<String>,
    pub touched: Vec<String>,
    pub scope: Vec<String>,
}

type RunRow = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn latest_run(tx: &rusqlite::Transaction, cid: i64) -> Option<RunRow> {
    tx.query_row(
        "SELECT start, joint, partner, partners, branch, base, worktree, touched FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1",
        [cid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)),
    )
    .ok()
}

fn lines(s: Option<String>) -> Vec<String> {
    s.unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

impl Store {
    /// The joint group `uid` sits in: same live start R plus joint links
    /// (`run.partner`/`partners`, both directions). Solo chunks return one side.
    pub fn status(&self, uid: &str) -> Result<Vec<Side>, String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        let (id, uid, _) = lookup(&tx, uid)?.ok_or_else(|| no_chunk(uid))?;
        let start: Option<String> = tx
            .query_row(
                "SELECT start FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .ok();
        let mut group: Vec<String> = vec![uid.clone()];
        if let Some(s) = &start {
            let mut same: Vec<String> = tx
                .prepare(
                    "SELECT DISTINCT c.uid FROM run r JOIN chunk c ON c.id = r.chunk WHERE r.start = ?1",
                )
                .map_err(err)?
                .query_map([s], |r| r.get(0))
                .map_err(err)?
                .collect::<Result<_, _>>()
                .map_err(err)?;
            group.append(&mut same);
        }
        for u in joint_of(&tx, &uid)? {
            if !group.contains(&u) {
                group.push(u);
            }
        }
        group.sort();
        group.dedup();
        let mut out = Vec::new();
        for u in group {
            let (cid, title, state, scope): (i64, String, String, Option<String>) = tx
                .query_row(
                    "SELECT id, title, state, scope FROM chunk WHERE uid = ?1",
                    [&u],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(err)?;
            let row: Option<RunRow> = latest_run(&tx, cid);
            let (run, joint, partner, partners, branch, base, worktree, touched) = match row {
                Some((run, joint, partner, partners, branch, base, worktree, touched)) => (
                    Some(run),
                    joint,
                    partner,
                    partners
                        .and_then(|s: String| serde_json::from_str::<Vec<String>>(&s).ok())
                        .unwrap_or_default(),
                    branch,
                    base,
                    worktree,
                    lines(touched),
                ),
                None => (None, None, None, Vec::new(), None, None, None, Vec::new()),
            };
            out.push(Side {
                uid: u,
                title,
                state,
                run,
                joint,
                partner,
                partners,
                branch,
                base,
                worktree,
                touched,
                scope: lines(scope),
            });
        }
        Ok(out)
    }
}

/// Every uid joint with `uid`: `run.partner` + `partners` JSON, both directions.
fn joint_of(tx: &rusqlite::Transaction, uid: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let rows: Vec<(Option<String>, Option<String>)> = tx
        .prepare(
            "SELECT r.partner, r.partners FROM run r JOIN chunk c ON c.id = r.chunk WHERE c.uid = ?1 AND r.joint IS NOT NULL",
        )
        .map_err(err)?
        .query_map([uid], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    for (p, ps) in rows {
        if let Some(p) = p {
            out.push(p);
        }
        if let Some(s) = ps
            && let Ok(v) = serde_json::from_str::<Vec<String>>(&s)
        {
            out.extend(v);
        }
    }
    let rev: Vec<String> = tx
        .prepare(
            "SELECT c.uid FROM run r JOIN chunk c ON c.id = r.chunk WHERE r.partner = ?1 OR r.partners LIKE ?2",
        )
        .map_err(err)?
        .query_map(
            rusqlite::params![uid, format!("%\"{uid}\"%")],
            |r| r.get(0),
        )
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    out.extend(rev);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::tests::{add, here, start};

    fn scoped(s: &mut Store, title: &str, scope: &str) -> String {
        let c = add(s, title, Some(title));
        s.conn
            .execute(
                "UPDATE chunk SET scope = ?2 WHERE uid = ?1",
                rusqlite::params![c, scope],
            )
            .unwrap();
        c
    }

    #[test]
    fn pair_shares_one_start_and_status_shows_both() {
        let mut s = Store::open_in_memory().unwrap();
        let a = scoped(&mut s, "a", "src/x.rs");
        let b = scoped(&mut s, "b", "src/x.rs");
        start(&mut s, &a, "/w1").unwrap();
        s.start(
            &b,
            &crate::Start {
                with: vec![a.clone()],
                joint: Some(crate::Joint::Pair),
                ..Default::default()
            },
            &here("/w2"),
        )
        .unwrap();
        for key in [&a, &b] {
            let sides = s.status(key).unwrap();
            let uids: Vec<_> = sides.iter().map(|x| x.uid.as_str()).collect();
            assert!(
                uids.contains(&a.as_str()) && uids.contains(&b.as_str()),
                "{uids:?}"
            );
        }
    }

    #[test]
    fn solo_chunk_is_one_side() {
        let mut s = Store::open_in_memory().unwrap();
        let a = scoped(&mut s, "a", "src/a.rs");
        let sides = s.status(&a).unwrap();
        assert_eq!(sides.len(), 1);
        assert_eq!(sides[0].uid, a);
    }
}
