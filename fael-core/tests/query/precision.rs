//! Push precision golden fixture (PLAN-fael-moat-token chunk 2). Two layers,
//! both through public fns: `push_tiered` (candidate + filter + tier, row id →
//! tier) and the final injected ids (`select` row cap, then `render` budget —
//! what the hook push actually says). Each case is a miss seen in a real
//! session (notes 01M3S446P, 01M3S6NAH) or a rung of the tier table.

use super::{ids, row};
use fael_core::*;

fn tiers(l: &Log, q: &[&str], al: &Aliases, read: bool) -> Vec<(String, usize)> {
    let q: Vec<String> = q.iter().map(|s| s.to_string()).collect();
    push_tiered(l, &q, al, read)
        .into_iter()
        .map(|(r, t)| (r.id[24..].to_string(), t))
        .collect()
}

/// What the hook push says for `q`: the row cap, then the token budget cut —
/// the same two steps `fael/src/hook/push.rs` runs.
fn injected(l: &Log, q: &[&str], al: &Aliases, read: bool) -> Vec<String> {
    let q: Vec<String> = q.iter().map(|s| s.to_string()).collect();
    let policy = PushPolicy {
        max_rows: 5,
        budget: 800,
        background: PUSH_BACKGROUND,
    };
    let sel = select(push_tiered(l, &q, al, read), &Focus::default(), &policy);
    let n = render(l, &sel.shown, policy.budget)
        .lines()
        .filter(|l| l.starts_with("- ["))
        .count();
    ids(&sel.shown[..n])
}

fn rid(n: u32) -> String {
    format!("A{:0>25}", n)
}

fn decision(n: u32, files: &[&str], key: Option<&str>, text: &str) -> Row {
    let mut r = row(&rid(n), "decision", files, key);
    r.text = text.into();
    r
}

fn log_of(rows: Vec<Row>) -> Log {
    Log {
        rows,
        closes: vec![],
        warnings: vec![],
    }
}

fn split_pairs() -> Aliases {
    Aliases::from_pairs(
        ["protocol", "state", "claude"]
            .iter()
            .map(|n| {
                (
                    "fael/src/hook.rs".to_string(),
                    format!("fael/src/hook/{n}.rs"),
                )
            })
            .collect(),
    )
}

/// The tier table: exact / zone / same-dir / shared-key, closed and
/// superseded hidden, issue before decision before note, freshest first.
#[test]
fn tier_ladder_read_and_edit() {
    let mut issue = row(&rid(20), "issue", &["src/a.rs"], None);
    issue.text = "login loops".into();
    let mut note = row(&rid(22), "note", &["src/a.rs"], None);
    note.text = "a note".into();
    let mut closed_dec = decision(27, &["src/a.rs"], None, "closed");
    closed_dec.extra.insert("closed".into(), "done".into());
    let mut newer = decision(29, &["src/a.rs"], None, "supersedes 28");
    newer.supersedes = Some(rid(28));
    let mut l = log_of(vec![
        issue,
        decision(21, &["src/a.rs"], Some("auth:x"), "exact, keyed"),
        note,
        decision(23, &["src/"], None, "zone row"),
        decision(24, &["src/b.rs"], None, "neighbour"),
        decision(25, &["lib/z.rs"], Some("auth:x"), "shares the key"),
        decision(26, &["lib/y.rs"], None, "unrelated"),
        closed_dec,
        decision(28, &["src/a.rs"], None, "superseded"),
        newer,
    ]);
    // a row named in `closes` is hidden too
    l.closes.push(Row::close("t-0000", &rid(26), "done"));
    let al = Aliases::default();
    let t = |read| tiers(&l, &["src/a.rs"], &al, read);
    let want = |rows: &[(&str, usize)]| {
        rows.iter()
            .map(|(i, t)| (i.to_string(), *t))
            .collect::<Vec<_>>()
    };
    // reads drop the same-dir ring (24)
    assert_eq!(
        t(true),
        want(&[
            ("20", 0),
            ("29", 0),
            ("23", 0),
            ("21", 0),
            ("22", 0),
            ("25", 2)
        ])
    );
    // edits keep it, between exact and shared-key
    assert_eq!(
        t(false),
        want(&[
            ("20", 0),
            ("29", 0),
            ("23", 0),
            ("21", 0),
            ("22", 0),
            ("24", 1),
            ("25", 2)
        ])
    );
}

/// Case "inherit" (01M3S446P): `hook.rs` was split into `hook/protocol.rs`,
/// `state.rs`, `claude.rs`; every child used to inherit the parent's rows
/// through the rename alias at the exact tier, though none says which child it
/// is about. fael never guesses the child: the push drops them (the pull,
/// `find --files <child>`, still expands the alias and reaches them).
#[test]
fn split_file_rows_do_not_reach_the_children() {
    let l = log_of(vec![
        decision(11, &["fael/src/hook.rs"], None, "filed before the split"),
        decision(12, &["fael/src/hook/state.rs"], None, "about state.rs"),
        decision(13, &["fael/src/hook.rs"], None, "also before the split"),
    ]);
    let al = split_pairs();
    assert!(tiers(&l, &["fael/src/hook/protocol.rs"], &al, true).is_empty());
    // an edit keeps only the genuine neighbour (12), at the same-dir tier
    assert_eq!(
        tiers(&l, &["fael/src/hook/protocol.rs"], &al, false),
        [("12".to_string(), 1)]
    );
    // the row filed on the child itself stays exact at that child
    assert_eq!(
        tiers(&l, &["fael/src/hook/state.rs"], &al, true),
        [("12".to_string(), 0)]
    );
    // the parent still exists: reading it reaches its own rows
    assert_eq!(
        tiers(&l, &["fael/src/hook.rs"], &al, true),
        [("13".to_string(), 0), ("11".to_string(), 0)]
    );
    // find expands across the split, every row the old path held is wanted
    let f = al.expand_all(&["fael/src/hook/protocol.rs".to_string()]);
    assert!(f.contains(&"fael/src/hook.rs".to_string()), "{f:?}");
}

/// A chain that ends in a split keeps the hops before it: `a → b`, then `b`
/// split into `c` and `d` — reading `b` still reaches `a`'s rows.
#[test]
fn rename_before_a_split_still_resolves() {
    let l = log_of(vec![decision(11, &["a.rs"], None, "filed at a")]);
    let al = Aliases::from_pairs(vec![
        ("a.rs".into(), "b.rs".into()),
        ("b.rs".into(), "c.rs".into()),
        ("b.rs".into(), "d.rs".into()),
    ]);
    assert_eq!(tiers(&l, &["b.rs"], &al, true), [("11".to_string(), 0)]);
    assert!(tiers(&l, &["c.rs"], &al, true).is_empty());
}

/// A plain one-to-one rename keeps its rows exact — the alias is the whole
/// point of following renames, and must not move with the split case.
#[test]
fn one_to_one_rename_stays_exact() {
    let l = log_of(vec![decision(
        11,
        &["old/x.rs"],
        None,
        "filed at the old path",
    )]);
    let al = Aliases::from_pairs(vec![("old/x.rs".into(), "new/x.rs".into())]);
    assert_eq!(tiers(&l, &["new/x.rs"], &al, true), [("11".to_string(), 0)]);
}

/// Case "hot-file flood" (01M3S6NAH): 30 exact-tier decisions on one doc, two
/// about kickoff. No structural signal says which row matches the edit, so
/// the freshest five were a guess — past `PUSH_HUB_ROWS` only a
/// `PUSH_HUB_PEEK` of them push off the Focus (issue push:hub-files): a
/// header with no row said nothing; the count line names `fael find --files`.
#[test]
fn hot_file_pushes_only_a_peek_off_focus() {
    let rows = (1..=30)
        .map(|n| {
            let text = if n == 2 || n == 5 {
                "kickoff ranks by freshness"
            } else {
                "some other topic"
            };
            decision(n, &["docs/architecture.md"], None, text)
        })
        .collect();
    let l = log_of(rows);
    let got = injected(&l, &["docs/architecture.md"], &Aliases::default(), false);
    assert_eq!(got.len(), PUSH_HUB_PEEK, "{got:?}");
}
