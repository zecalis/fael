//! The consolidate ask (PLAN-fael-context-loop chunk 3): an edit whose file
//! has `MIN_ROWS` or more open decision/note rows in the agent's context is
//! asked once, with the ids, whether they are one matter, and the commands to
//! file it once. fael only asks; the agent decides what the rows mean.

use super::changed::{Ask, own_row};
use super::say::{Kind, Line};
use crate::core;

/// Frozen from the in-context edit events before this code was written
/// (decision `plan:fael-context-loop:consolidate-n`): more than one push cap
/// (5) of rows, on about one edit in nine. Cut, never retuned, if the kind
/// misses the 20% yield bar.
pub(crate) const MIN_ROWS: usize = 6;

/// Rows named in the ask; the rest are in context already.
const NAMED: usize = 3;

/// The first edited file with `MIN_ROWS` open tier-0 decision/note rows in
/// context, not the user's call and not this session's own, as one `Merge`
/// line. `t0` is the edit's tier-0 rows, `said` the rows this push said.
pub(crate) fn merge_line(
    ask: &Ask,
    t0: &[(&core::Row, usize)],
    said: &[&core::Row],
) -> Option<Line> {
    ask.files.iter().find_map(|f| {
        let rows: Vec<&core::Row> = t0
            .iter()
            .filter(|(r, tier)| {
                *tier == 0
                    && matches!(r.kind.as_str(), "decision" | "note")
                    && !r.from_user()
                    && r.files.contains(f)
                    && (ask.told.contains(&r.id) || said.iter().any(|s| s.id == r.id))
                    && (ask.session.is_empty() || !own_row(r, ask.session))
            })
            .map(|(r, _)| *r)
            .collect();
        (rows.len() >= MIN_ROWS).then(|| line(ask.log, f, &rows))
    })
}

fn line(log: &core::Log, file: &str, rows: &[&core::Row]) -> Line {
    let ab = core::abbrev(log);
    let named: Vec<&core::Row> = rows.iter().take(NAMED).copied().collect();
    let ids: Vec<&str> = named.iter().map(|r| ab.short(&r.id)).collect();
    let kind = if named.iter().all(|r| r.kind == "note") {
        "note"
    } else {
        "decision"
    };
    let text = format!(
        "fael: {file} has {} open decision/note rows in front of you, among them {} — one matter? file it once: `fael add {kind} \"<the one rule>\" --files {file} --supersedes {}` · then `fael close <id> \"merged into <new id>\"` for the others\n",
        rows.len(),
        ids.join(" · "),
        ids[0]
    );
    Line {
        kind: Kind::Merge {
            file: file.to_string(),
            ids: named.iter().map(|r| r.id.clone()).collect(),
        },
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::super::say::{Kind, Line, Outbox};
    use super::super::state::lock_seen;
    use crate::core::est_tokens;

    fn ask() -> Line {
        Line {
            kind: Kind::Ask {
                ids: vec!["01ASK".into()],
                issue: false,
            },
            text: "fael: done with one? fael close 01ASK \"<why>\"\n".into(),
        }
    }

    fn merge() -> Line {
        Line {
            kind: Kind::Merge {
                file: "a.rs".into(),
                ids: vec!["01MERGE".into()],
            },
            text: "fael: a.rs has 6 open rows — fael add decision \"x\" --supersedes 01MERGE\n"
                .into(),
        }
    }

    fn seen() -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("fael-merge-{}", crate::core::ulid()))
            .join("s.seen")
    }

    /// Over budget the consolidate ask goes before the edit ask, and a cut
    /// line spends no key.
    #[test]
    fn over_the_budget_merge_goes_before_ask() {
        let p = seen();
        let mut out = Outbox::open(lock_seen(&p));
        out.say_within(est_tokens(&ask().text), vec![ask(), merge()]);
        let said = out.reply().context().unwrap_or("").to_string();
        assert!(
            said.contains("fael close 01ASK") && !said.contains("01MERGE"),
            "{said}"
        );
        assert!(!std::fs::read_to_string(&p).unwrap().contains("~merge:"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// One edit ask per user turn, shared with the merge ask: the ask wins
    /// the turn, the merge keeps its key and is said in the next.
    #[test]
    fn the_ask_wins_the_turn_and_the_merge_waits() {
        let p = seen();
        let in_turn = |t: &str, ls: Vec<Line>| {
            let mut out = Outbox::open(lock_seen(&p)).in_turn(Some(t.into()));
            ls.into_iter().for_each(|l| out.say(l));
            out.reply().context().map(String::from)
        };
        let t1 = in_turn("t1", vec![ask(), merge()]).unwrap();
        assert!(t1.contains("01ASK") && !t1.contains("01MERGE"), "{t1}");
        let t2 = in_turn("t2", vec![merge()]).unwrap();
        assert!(t2.contains("01MERGE"), "{t2}");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
