//! The gone-check ask (PLAN-fael-experience-loop chunk 3): an edit of a file
//! that a closed issue sits on, whose close text names a backticked path with
//! nothing behind it any more, says so once per issue and session. fael does
//! not judge the check — it only finds the pointer dead; the agent restores
//! it, or files what is unguarded now.

use super::changed::Ask;
use super::say::{Kind, Line};
use crate::core;

/// Paths named in the line; the rest are one `fael find <id>` away.
const NAMED: usize = 2;

/// The edit-time asks beyond the edit hint: the consolidate ask, then the
/// gone-check ask. A read (`edit` false) gets neither.
pub(crate) fn asks(
    ask: &Ask,
    t0: &[(&core::Row, usize)],
    said: &[&core::Row],
    edit: bool,
) -> Vec<Line> {
    let merge = super::merge::merge_line(ask, t0, said);
    merge.into_iter().chain(check_line(ask, edit)).collect()
}

/// Closed issues examined per edit, newest first: a hub file with hundreds
/// of them would otherwise cost every edit a close scan and a stat each.
// ponytail: newest 50 only, a gone check on an older issue goes unsaid
const SCAN: usize = 50;

/// The newest closed issue on an edited file whose close names a gone path,
/// as one `Check` line. `edit` is false off an edit: a read gets no line.
pub(crate) fn check_line(ask: &Ask, edit: bool) -> Option<Line> {
    if !edit {
        return None;
    }
    let on_file =
        |r: &&core::Row| r.kind == "issue" && ask.files.iter().any(|f| r.files.contains(f));
    let (r, gone) = ask
        .log
        .rows
        .iter()
        .rev()
        .filter(on_file)
        .take(SCAN)
        .find_map(|r| {
            let gone = core::stale_close_refs(ask.root, ask.log, r, ask.al);
            (!gone.is_empty()).then_some((r, gone))
        })?;
    let file = ask.files.iter().find(|f| r.files.contains(f))?;
    let named: Vec<String> = gone.iter().take(NAMED).map(|p| format!("`{p}`")).collect();
    let id = core::abbrev(ask.log).short(&r.id).to_string();
    Some(Line {
        kind: Kind::Check { id: r.id.clone() },
        text: format!(
            "fael: {id} was closed pointing at {}, now gone — restore it, or file what is unguarded: `fael add issue \"<what regressed>\" --files {file} --supersedes {id}`\n",
            named.join(" · ")
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::Instant;

    /// `n` closed issues on `src/a.rs` (each closed pointing at a check that
    /// is there) among `n` rows elsewhere — a hub file's worst case.
    fn hub(n: usize) -> core::Log {
        let mut log = core::Log::default();
        for i in 0..n {
            let id = format!("01HUB{i:08}");
            log.rows.push(core::Row {
                id: id.clone(),
                ts: "2026-10-01T00:00:00Z".into(),
                kind: "issue".into(),
                text: format!("problem {i}"),
                files: vec!["src/a.rs".into()],
                ..core::Row::default()
            });
            log.closes.push(core::Row {
                id: format!("01CLS{i:08}"),
                ts: "2026-10-02T00:00:00Z".into(),
                text: "guarded by `src/a.rs`".into(),
                reference: Some(id),
                ..core::Row::default()
            });
            log.rows.push(core::Row {
                id: format!("01OTH{i:08}"),
                kind: "note".into(),
                files: vec![format!("src/other{i}.rs")],
                ..core::Row::default()
            });
        }
        log
    }

    fn line(log: &core::Log) -> (Option<Line>, std::time::Duration) {
        let (al, set) = (core::Aliases::default(), HashSet::new());
        let files = vec!["src/a.rs".to_string()];
        let ask = Ask {
            log,
            root: std::path::Path::new("."),
            files: &files,
            al: &al,
            session: "",
            told: &set,
            hinted: &set,
        };
        let t = Instant::now();
        (check_line(&ask, true), t.elapsed())
    }

    /// PLAN-fael-experience-loop §4: the push path stays ≤ 5 ms. A hub file
    /// of 5000 closed issues in a 10000-row log, in a debug build, is held to
    /// ten times that — the said and the silent case alike.
    #[test]
    fn a_hub_of_closed_issues_stays_inside_the_push_budget() {
        let mut log = hub(5000);
        let (got, silent) = line(&log);
        assert!(got.is_none());
        // the newest issue's close now points at a path that is gone
        log.closes.last_mut().unwrap().text = "moved to `t/gone.sh`".into();
        let (got, said) = line(&log);
        assert!(got.unwrap().text.contains("`t/gone.sh`"));
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        assert!(
            ms(silent) < 50.0 && ms(said) < 50.0,
            "{silent:?} / {said:?}"
        );
    }

    /// Only the newest `SCAN` closed issues are looked at: a gone check on
    /// an older one goes unsaid, one inside the window is said.
    #[test]
    fn only_the_newest_closed_issues_are_examined() {
        let gone = |log: &mut core::Log, i: usize| {
            log.closes[i].text = "moved to `t/gone.sh`".into();
        };
        let mut log = hub(SCAN + 10);
        gone(&mut log, 0); // the oldest
        assert!(line(&log).0.is_none());
        gone(&mut log, 10 + 5); // within the newest SCAN
        assert!(line(&log).0.is_some());
    }
}
