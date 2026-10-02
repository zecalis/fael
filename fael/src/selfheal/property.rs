//! Chunk 2's invariants as generated unit tests
//! (PLAN-fael-selfheal-verdict).
//!
//! The corpus (chunk 3) catches cases we have seen; these catch cases we have
//! not. Every invariant runs generated logs through `decide`/`heal` with a
//! fixed seed, so a failure prints seed + case and replays exactly.
//! Hand-rolled LCG — the repo carries no proptest/quickcheck, and a 15-line
//! generator is not worth a dependency.
//!
//! Scope notes, read before "fixing" a failure by changing behaviour:
//! - invariant 1 covers the automatic classes only (Identity, Heuristic).
//!   Explicit is the caller's own intent — text naming a row, like a
//!   `--supersedes` flag passing through core — and may close any open row
//!   the caller names.
//! - invariant 4 compares canonicalised verdicts (id lists sorted): the
//!   decision never depends on log order. Listings over five targets keep log
//!   order through the render cut like before (pre-existing, out of scope).

use super::decide::{Verdict, decide, evaluate};
use super::evidence::open_rows;
use crate::core;

const SEED: u64 = 0xFAE1_5EED_0002;
const CASES: usize = 200;

/// Minimal LCG (Numerical Recipes constants): fixed seed, exact replay.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn one_of<'a>(&mut self, xs: &'a [&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }

    fn chance(&mut self, p: u64) -> bool {
        self.next() % 10 < p
    }
}

fn stamp() -> core::Stamp {
    core::Stamp {
        by: "me".to_string(),
        branch: Some("main".to_string()),
        sha: None,
    }
}

fn mk(
    id: usize,
    by: &str,
    kind: &str,
    text: &str,
    files: Vec<String>,
    key: Option<String>,
) -> core::Row {
    let mut r = core::Row::new(by, kind, text, files);
    r.id = format!("01GEN{id:022}");
    r.key = key;
    r
}

/// One generated scene: 0–4 open rows plus the row being added.
fn gen_case(g: &mut Lcg, n: usize) -> (core::Log, core::Stamp, core::Row) {
    const KINDS: [&str; 4] = ["note", "note", "decision", "issue"];
    const WRITERS: [&str; 3] = ["me", "me", "other"];
    const TEXTS: [&str; 5] = [
        "first pass",
        "second pass",
        "broken counter",
        "stale cache",
        "about a",
    ];
    const FILES: [&str; 3] = ["src/a.rs", "src/b.rs", "src/c.rs"];
    let st = stamp();
    let mut rows = vec![];
    for i in 0..g.below(5) {
        let files: Vec<String> = FILES
            .iter()
            .filter(|_| g.chance(5))
            .map(|s| s.to_string())
            .collect();
        let mut r = mk(
            n * 10 + i,
            g.one_of(&WRITERS),
            g.one_of(&KINDS),
            g.one_of(&TEXTS),
            files,
            g.chance(4).then(|| g.one_of(&["k:a", "k:b"]).to_string()),
        );
        if g.chance(6) {
            r.extra.insert(
                "branch".to_string(),
                serde_json::Value::String(g.one_of(&["main", "feature"]).to_string()),
            );
        }
        rows.push(r);
    }
    let files: Vec<String> = FILES
        .iter()
        .filter(|_| g.chance(6))
        .map(|s| s.to_string())
        .collect();
    let mut row = mk(
        n * 10 + 9,
        "me",
        g.one_of(&KINDS),
        g.one_of(&TEXTS),
        files,
        g.chance(4).then(|| g.one_of(&["k:a", "k:b"]).to_string()),
    );
    if g.chance(8) {
        row.extra.insert(
            "branch".to_string(),
            serde_json::Value::String("main".to_string()),
        );
    }
    (
        core::Log {
            rows,
            ..Default::default()
        },
        st,
        row,
    )
}

/// The verdict with every id list sorted: what the decision is, regardless
/// of the order the log happened to list the rows in.
fn canon(v: &Verdict) -> Verdict {
    if let Verdict::CrossKey { inner, old, new } = v {
        return Verdict::CrossKey {
            inner: Box::new(canon(inner)),
            old: old.clone(),
            new: new.clone(),
        };
    }
    let mut out = v.clone();
    let sort = |xs: &mut Vec<String>| xs.sort();
    match &mut out {
        Verdict::TextAct { also, .. } | Verdict::KeyAct { also, .. } => sort(also),
        Verdict::TextHold { targets }
        | Verdict::KeyMany { targets }
        | Verdict::FilesMany { targets }
        | Verdict::KeyIssueKept { targets } => sort(targets),
        _ => {}
    }
    out
}

/// Read through one `CrossKey` layer: the exposure wrapper never changes
/// what the decision closes, so every invariant below sees the act inside.
fn eff(v: &Verdict) -> &Verdict {
    match v {
        Verdict::CrossKey { inner, .. } => inner,
        _ => v,
    }
}

fn target_row<'a>(log: &'a core::Log, id: &str) -> &'a core::Row {
    log.rows.iter().find(|r| r.id == id).unwrap()
}

/// First, the automatic classes never close another writer's row. (Explicit is
/// the caller's own intent and is exempt — see the module doc.)
#[test]
fn automatic_acts_close_only_my_own_rows() {
    let mut g = Lcg(SEED);
    for n in 0..CASES {
        let (log, st, row) = gen_case(&mut g, n);
        let open = open_rows(&log);
        match eff(&decide(&log, &open, &st, &row, None)) {
            Verdict::KeyAct { target, .. } | Verdict::FilesAct { target } => {
                let by = &target_row(&log, target).by;
                assert_eq!(by, "me", "seed {SEED:#x} case {n}: closed {target} of {by}");
            }
            _ => {}
        }
    }
}

/// Sixth, shared files prove relatedness, not replacement: a files act never
/// hides a keyed row, and a keyed new row is never the cause of one. A key
/// names a topic; the files guess only fires where no topic exists to lose.
#[test]
fn files_never_hide_a_keyed_topic() {
    let mut g = Lcg(SEED + 5);
    for n in 0..CASES {
        let (log, st, row) = gen_case(&mut g, n);
        let open = open_rows(&log);
        if let Verdict::FilesAct { target } = eff(&decide(&log, &open, &st, &row, None)) {
            let old = &target_row(&log, target).key;
            assert!(
                old.is_none() && row.key.is_none(),
                "seed {SEED:#x} case {n}: files hid {target} ({old:?}) for key {:?}",
                row.key
            );
        }
    }
}

/// Second, at most one row goes: `evaluate` names a single open id, or none.
/// The mode only moves the exposure (`warning:` vs info vs silent), never
/// the supersede — one mode covers all three.
#[test]
fn at_most_one_row_is_superseded() {
    let mut g = Lcg(SEED + 1);
    for n in 0..CASES {
        let (log, st, row) = gen_case(&mut g, n);
        let h = evaluate(&log, &st, &row, None, core::CrossKey::Warn).heal;
        if let Some(id) = h.supersedes {
            assert!(
                log.rows.iter().any(|r| r.id == id),
                "seed {SEED:#x} case {n}: supersedes unknown {id}"
            );
        }
    }
}

/// Third, a keyless row carries no identity: the Identity class stays silent,
/// so whatever fael later guesses (auto-key) can never be the cause of an act.
#[test]
fn keyless_rows_never_act_by_key() {
    let mut g = Lcg(SEED + 2);
    for n in 0..CASES {
        let (log, st, mut row) = gen_case(&mut g, n);
        row.key = None;
        let open = open_rows(&log);
        let v = decide(&log, &open, &st, &row, None);
        assert!(
            !matches!(
                v,
                Verdict::KeyAct { .. }
                    | Verdict::KeyMany { .. }
                    | Verdict::KeyOtherWriter { .. }
                    | Verdict::KeyIssueKept { .. }
            ),
            "seed {SEED:#x} case {n}: keyless row reached {v:?}"
        );
    }
}

/// Fourth, log order is presentation, not input: shuffling the open rows decides
/// the same verdict (compared canonicalised — see the module doc).
#[test]
fn verdict_ignores_log_order() {
    let mut g = Lcg(SEED + 3);
    for n in 0..CASES {
        let (log, st, row) = gen_case(&mut g, n);
        let mut idx: Vec<usize> = (0..log.rows.len()).collect();
        for i in (1..idx.len()).rev() {
            idx.swap(i, g.below(i + 1));
        }
        let open: Vec<&core::Row> = open_rows(&log);
        let shuffled: Vec<&core::Row> = idx.iter().map(|&i| open[i]).collect();
        let a = canon(&decide(&log, &open, &st, &row, None));
        let b = canon(&decide(&log, &shuffled, &st, &row, None));
        assert_eq!(
            a, b,
            "seed {SEED:#x} case {n}: order changed {a:?} to {b:?}"
        );
    }
}

/// Fifth, hold — and every kept-open — files the row and touches nothing else.
#[test]
fn hold_and_kept_open_supersede_nothing() {
    let mut g = Lcg(SEED + 4);
    for n in 0..CASES {
        let (log, st, row) = gen_case(&mut g, n);
        let open = open_rows(&log);
        let v = decide(&log, &open, &st, &row, None);
        if matches!(
            eff(&v),
            Verdict::TextHold { .. }
                | Verdict::KeyMany { .. }
                | Verdict::FilesMany { .. }
                | Verdict::KeyIssueKept { .. }
                | Verdict::KeyOtherWriter { .. }
        ) {
            let h = evaluate(&log, &st, &row, None, core::CrossKey::Warn).heal;
            assert_eq!(
                h.supersedes, None,
                "seed {SEED:#x} case {n}: {v:?} superseded {:?}",
                h.supersedes
            );
        }
    }
}
