//! The agent's half of the contract (SPEC §1): `wait`, `after`, `done` — fenced to the
//! worktree of the chunk's latest run, each ending its live run in the same transaction —
//! and the Stop hook's stamp on the live run of its own worktree.

use super::State;
use super::board_math::under;
use super::chunk::{Here, add_after, end_live, fence, is_date, row, scope, set, tx, uid_id};
use super::shared::unshared;
use super::store::{Store, err};
use rusqlite::{Transaction, params};

/// `chunk done --verdict` words (plan fael-intake, contract §done --verdict).
const VERDICTS: &[&str] = &["fix", "answer", "consider", "plan", "decline"];

/// Ends the live run; from the run's own worktree it keeps the branch the agent is on now
/// (it may have switched since start), so board and the app's Talk name the real one.
fn end(tx: &Transaction, chunk: i64, here: &Here) -> Result<usize, String> {
    if let Some(b) = here.branch {
        tx.execute(
            "UPDATE run SET branch = ?1 WHERE chunk = ?2 AND ended IS NULL AND worktree = ?3",
            params![b, chunk, here.worktree],
        )
        .map_err(err)?;
    }
    end_live(tx, chunk, here.now)
}

/// A chunk leaves `waiting`: its question, date and options go (01M4NWFS).
pub(super) fn unwait(tx: &Transaction, chunk: i64) -> Result<(), String> {
    tx.execute(
        "UPDATE chunk SET wait_on = NULL, wait_text = NULL, wait_until = NULL,
           wait_options = NULL WHERE id = ?1",
        [chunk],
    )
    .map(drop)
    .map_err(err)
}

/// `Store::outside`: the declared scope, and each path outside it with the chunk that holds it.
pub type Outside = (Vec<String>, Vec<(String, Option<String>)>);

/// `done --out`: copy the output for run R, return where it went.
pub type Copy<'a> = dyn FnMut(&str) -> Result<String, String> + 'a;
/// A live run row: chunk uid, the branch and worktree it recorded.
pub type RunRow = (String, Option<String>, Option<String>);

impl Store {
    /// `fael chunk wait --on owner|data "<question>" [--until D] [--option A …]`: the run
    /// ends, the question stays; `options` are the answers the owner picks from (01M4NWFS).
    pub fn wait(
        &mut self,
        uid: &str,
        on: &str,
        text: &str,
        until: Option<&str>,
        options: &[String],
        here: &Here,
    ) -> Result<(), String> {
        if !matches!(on, "owner" | "data") {
            return Err(format!("rejected: --on {on} — owner or data"));
        }
        if !options.is_empty() && on != "owner" {
            return Err("rejected: --option needs --on owner".into());
        }
        let options = (!options.is_empty())
            .then(|| serde_json::to_string(options))
            .transpose()
            .map_err(|e| e.to_string())?;
        if until.is_some_and(|u| !is_date(u)) {
            return Err("rejected: --until takes a date YYYY-MM-DD".into());
        }
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        fence(&tx, &r, here)?;
        set(&tx, &r, State::Waiting, "agent", Some(text), here.now)?;
        tx.execute(
            "UPDATE chunk SET wait_on = ?1, wait_text = ?2, wait_until = ?3, wait_options = ?4
             WHERE id = ?5",
            params![on, text, until, options, r.id],
        )
        .map_err(err)?;
        end(&tx, r.id, here)?;
        tx.commit().map_err(err)
    }

    /// `fael chunk after <uid> <other> "<why>"`: needs `other` first. A running chunk goes
    /// back to `open` and its run ends; `blocked` follows from the edge (SPEC §2).
    pub fn after(&mut self, uid: &str, other: &str, why: &str, here: &Here) -> Result<(), String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        fence(&tx, &r, here)?;
        let pid: i64 = tx
            .query_row("SELECT plan FROM chunk WHERE id = ?1", [r.id], |q| q.get(0))
            .map_err(err)?;
        let dst = uid_id(&tx, pid, other)?;
        if dst == r.id {
            return Err("rejected: a chunk cannot wait on itself".into());
        }
        add_after(&tx, r.id, dst)?;
        let to = if r.state == State::Running {
            end(&tx, r.id, here)?;
            State::Open
        } else if r.state.terminal() || r.state == State::Review {
            return Err(format!(
                "rejected: chunk {uid} is {} — an `after` only holds back work not yet done",
                r.state.as_str()
            ));
        } else {
            r.state
        };
        set(
            &tx,
            &r,
            to,
            "agent",
            Some(&format!("after {other}: {why}")),
            here.now,
        )?;
        tx.commit().map_err(err)
    }

    /// `fael chunk done "<handoff>" [--pr N | --out <path>]`: with a PR → done (the owner's
    /// push pr is the ok; a later problem is a new chunk or issue), else review. `out` copies the
    /// output for this run R (inside the transaction: a failed copy changes nothing) and
    /// returns the path kept in `run.out`.
    pub fn done(
        &mut self,
        uid: &str,
        handoff: &str,
        pr: Option<i64>,
        out: Option<&mut Copy<'_>>,
        here: &Here,
    ) -> Result<(), String> {
        self.done_verdict(uid, handoff, pr, out, None, here)
    }

    /// `done` with `--verdict` (plan fael-intake): only a ticket chunk takes one, kept on its
    /// run for the board's `ticket.verdict`.
    pub fn done_verdict(
        &mut self,
        uid: &str,
        handoff: &str,
        pr: Option<i64>,
        out: Option<&mut Copy<'_>>,
        verdict: Option<&str>,
        here: &Here,
    ) -> Result<(), String> {
        if verdict.is_some_and(|v| !VERDICTS.contains(&v)) {
            return Err(format!("rejected: --verdict is {}", VERDICTS.join("|")));
        }
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        fence(&tx, &r, here)?;
        let ticket: Option<String> = tx
            .query_row("SELECT ticket_ref FROM chunk WHERE id = ?1", [r.id], |q| {
                q.get(0)
            })
            .map_err(err)?;
        if verdict.is_some() && ticket.is_none() {
            return Err("rejected: --verdict is for a ticket chunk".into());
        }
        let to = if pr.is_some() {
            State::Done
        } else {
            State::Review
        };
        set(&tx, &r, to, "agent", Some(handoff), here.now)?;
        let (run, start): (i64, String) = tx
            .query_row(
                "SELECT id, start FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1",
                [r.id],
                |q| Ok((q.get(0)?, q.get(1)?)),
            )
            .map_err(err)?;
        let out = out.map(|f| f(&start)).transpose()?;
        tx.execute(
            "UPDATE run SET pr = COALESCE(?1, pr), out = COALESCE(?2, out),
               verdict = COALESCE(?3, verdict) WHERE id = ?4",
            params![pr, out, verdict, run],
        )
        .map_err(err)?;
        end(&tx, r.id, here)?;
        tx.commit().map_err(err)
    }

    /// `chunk done` (issue 01M4RRD1K): the branch's real diff becomes the chunk's scope, so a
    /// review or open-PR chunk holds the files it touched, not the ones it declared. No diff
    /// keeps the declared scope.
    pub fn claim(&mut self, uid: &str, files: &[String]) -> Result<(), String> {
        // ponytail: a path the scope rule refuses (`app/[id]/x`) is left out, not widened to its dir
        let ok: Vec<String> = files
            .iter()
            .filter(|f| scope(std::slice::from_ref(f)).is_ok())
            .cloned()
            .collect();
        let Some(s) = scope(&ok)? else {
            return Ok(());
        };
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        tx.execute(
            "UPDATE chunk SET scope = ?1, touched = ?1 WHERE id = ?2",
            params![s, r.id],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    /// The branch `done` diffs (issue 01M4TFB5): run `run`'s (`--run R`), else the chunk's
    /// latest run's — not the cwd's, so a lead's done per lane from the wave branch claims
    /// only that lane. None = no run or no branch recorded.
    pub fn run_branch(&self, uid: &str, run: Option<&str>) -> Option<String> {
        self.conn
            .query_row(
                "SELECT r.branch FROM run r JOIN chunk c ON c.id = r.chunk
                 WHERE c.uid = ?1 AND (?2 IS NULL OR r.start = ?2) ORDER BY r.id DESC LIMIT 1",
                params![uid, run],
                |q| q.get(0),
            )
            .ok()
            .flatten()
    }

    /// Start `run`'s live rows (a pair is one start): uid, branch, worktree.
    pub fn run_rows(&self, run: &str) -> Result<Vec<RunRow>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT c.uid, r.branch, r.worktree FROM run r JOIN chunk c ON c.id = r.chunk
                 WHERE r.start = ?1 AND r.ended IS NULL",
            )
            .map_err(err)?;
        q.query_map([run], |q| Ok((q.get(0)?, q.get(1)?, q.get(2)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)
    }

    /// `chunk.touched` when the run `done` finishes had ended before it: `run end` claimed
    /// the lane's own diff then, before a lead rebased it onto the lanes landed before it.
    pub fn claimed_at_end(&self, uid: &str, run: Option<&str>) -> Option<Vec<String>> {
        let (ended, touched): (Option<String>, Option<String>) = self
            .conn
            .query_row(
                "SELECT r.ended, c.touched FROM run r JOIN chunk c ON c.id = r.chunk
                 WHERE c.uid = ?1 AND (?2 IS NULL OR r.start = ?2) ORDER BY r.id DESC LIMIT 1",
                params![uid, run],
                |q| Ok((q.get(0)?, q.get(1)?)),
            )
            .ok()?;
        ended?;
        Some(touched?.lines().map(String::from).collect())
    }

    /// HEAD at `uid`'s first start that recorded one: a restart after a wait keeps the WIP
    /// commits of the runs before it. None = no run recorded one (older runs, `--no-worktree`).
    pub fn run_base(&self, uid: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT r.base FROM run r JOIN chunk c ON c.id = r.chunk
                 WHERE c.uid = ?1 AND r.base IS NOT NULL ORDER BY r.id LIMIT 1",
                [uid],
                |q| q.get(0),
            )
            .ok()
    }

    /// `done`'s diff against `uid`'s declared scope, before `claim` replaces it: the declared
    /// scope (its pair's with it), then each path not under it with the `plan/label` of another unfinished chunk
    /// whose scope covers it (board's prefix rule), None = unowned. A `shared` path is
    /// nobody's and left out; no declared scope = nothing to compare, nothing said.
    pub fn outside(
        &self,
        uid: &str,
        files: &[String],
        shared: &[String],
    ) -> Result<Outside, String> {
        let lines = |s: Option<String>| -> Vec<String> {
            s.unwrap_or_default().lines().map(String::from).collect()
        };
        // a pair is one run's work: its partners' scopes are declared too
        let declared: Vec<String> = self
            .conn
            .prepare(
                "SELECT c.scope FROM chunk c WHERE c.uid = ?1 OR c.id IN (
                   SELECT e.dst FROM edge e JOIN chunk s ON s.id = e.src
                   WHERE s.uid = ?1 AND e.kind = 'pair'
                   UNION SELECT e.src FROM edge e JOIN chunk d ON d.id = e.dst
                   WHERE d.uid = ?1 AND e.kind = 'pair')
                 ORDER BY c.seq",
            )
            .map_err(err)?
            .query_map([uid], |q| q.get(0))
            .map_err(err)?
            .collect::<Result<Vec<Option<String>>, _>>()
            .map_err(err)?
            .into_iter()
            .flat_map(lines)
            .collect();
        let others: Vec<(String, Vec<String>)> = self
            .conn
            .prepare(
                "SELECT p.name || '/' || COALESCE(c.label, c.uid), c.scope FROM chunk c
                 JOIN plan p ON p.id = c.plan WHERE c.uid != ?1 AND c.scope IS NOT NULL
                 AND c.state NOT IN ('done', 'replaced', 'dropped') ORDER BY c.seq",
            )
            .map_err(err)?
            .query_map([uid], |q| Ok((q.get(0)?, q.get(1)?)))
            .map_err(err)?
            .map(|r| r.map(|(n, s)| (n, unshared(&lines(Some(s)), shared))))
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let out = files
            .iter()
            .filter(|f| !declared.is_empty() && !declared.iter().any(|d| under(f, d)))
            .filter(|f| !shared.iter().any(|s| under(f, s)))
            .map(|f| {
                let owner = others.iter().find(|(_, sc)| sc.iter().any(|d| under(f, d)));
                (f.clone(), owner.map(|(n, _)| n.clone()))
            })
            .collect();
        Ok((declared, out))
    }

    /// `fael chunk note <uid> "<text>"`: a word for whoever starts the chunk next — from any
    /// worktree, the owner too; state and approval stay (unlike `edit`), the next brief prints it.
    pub fn note(&mut self, uid: &str, text: &str, now: &str) -> Result<(), String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        if r.state.terminal() {
            return Err(format!(
                "rejected: chunk {uid} is {} — a note reaches only a chunk still to run; `fael add note` keeps it in the ledger",
                r.state.as_str()
            ));
        }
        set(&tx, &r, r.state, "note", Some(text), now)?;
        tx.commit().map_err(err)
    }

    /// Live runs in a worktree, `(uid, title, R)` — what a resumed session holds.
    pub fn held(&self, worktree: &str) -> Result<Vec<(String, String, String)>, String> {
        self.conn
            .prepare(
                "SELECT c.uid, c.title, r.start FROM run r JOIN chunk c ON c.id = r.chunk
                 WHERE r.worktree = ?1 AND r.ended IS NULL ORDER BY c.seq",
            )
            .map_err(err)?
            .query_map([worktree], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)
    }

    /// The Stop hook (SPEC §7): the live run of its own worktree gets `last_seen`, and
    /// `session` when empty. A run naming another session is left alone; an ended run is
    /// never written.
    pub fn seen(&mut self, worktree: &str, session: &str, now: &str) -> Result<usize, String> {
        self.conn
            .execute(
                "UPDATE run SET session = COALESCE(session, ?2), last_seen = ?3
                 WHERE worktree = ?1 AND ended IS NULL AND (session IS NULL OR session = ?2)",
                params![worktree, session, now],
            )
            .map_err(err)
    }

    /// The Stop hook in any cwd: the live runs `chunk start` bound to this session.
    pub fn seen_session(&mut self, session: &str, now: &str) -> Result<usize, String> {
        self.conn
            .execute(
                "UPDATE run SET last_seen = ?2 WHERE session = ?1 AND ended IS NULL",
                params![session, now],
            )
            .map_err(err)
    }

    /// `fael run beat R`: the launcher's heartbeat on start R's live rows (decision
    /// 01M4MRS8) — a headless run stops once, at its end. None left is fine.
    pub fn beat(&mut self, run: &str, now: &str) -> Result<usize, String> {
        self.conn
            .execute(
                "UPDATE run SET last_seen = ?2 WHERE start = ?1 AND ended IS NULL",
                params![run, now],
            )
            .map_err(err)
    }
}
