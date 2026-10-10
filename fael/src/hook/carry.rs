//! The carry-back line (PLAN-fael-experience-loop chunk 6): an edit of a file
//! that a closed issue sits on, closed with its fix named (a sha or `(#N)`),
//! puts that pair — what broke, how it was fixed — in front of the agent,
//! once per issue and session. A closed row never pushes (`push_tiered`), so
//! without it the fix is written down and never read again. fael does not
//! judge whether the old bug applies; the agent reads it and decides. A fix
//! not found on this checkout or the default branch is skipped (`reached`):
//! a new close needs a commit there citing it, an old one a sha that landed.

use super::changed::Ask;
use super::say::{Kind, Line};
use crate::core;

/// The close text shown, in chars; the rest is one `fael find <id>` away.
const CLOSE_CHARS: usize = 160;

/// Fixed issues whose fix is looked up in git per edit (two spawns each at
/// most, `fix_reached`): past the newest few, an unmerged fix stays silent, not slow.
const REACHED: usize = 3;

/// The newest closed, not superseded, issue on an edited file whose fix
/// reached main (`fix_close`, then `fix_reached`), as one `Carry` line — unless it is `skip` (the gone-check
/// line already names it). Only the newest per file: a file with thirty
/// fixed bugs says one, not one per turn. `edit` is false off an edit.
pub(crate) fn carry_line(ask: &Ask, edit: bool, skip: Option<&str>) -> Option<Line> {
    if !edit {
        return None;
    }
    let (closed, gone) = (core::closed(ask.log), core::superseded(ask.log));
    let on_file =
        |r: &&core::Row| r.kind == "issue" && ask.files.iter().any(|f| r.files.contains(f));
    let (r, (fix, _)) = ask
        .log
        .rows
        .iter()
        .rev()
        .filter(on_file)
        .filter(|r| closed.contains(r.id.as_str()) && !gone.contains(r.id.as_str()))
        .take(super::check::SCAN)
        .filter_map(|r| core::fix_close(ask.log, r).map(|t| (r, t)))
        .take(REACHED)
        // one said this session is spent anyway (the Outbox drops it): no spawn
        .find(|(r, (t, new))| {
            ask.hinted.contains(&format!("carry:{}", r.id))
                || super::reached::fix_reached(ask.root, ask.log, &r.id, t, *new) == Some(true)
        })?;
    if skip == Some(r.id.as_str()) {
        return None;
    }
    let file = ask.files.iter().find(|f| r.files.contains(f))?;
    let id = core::abbrev(ask.log).short(&r.id).to_string();
    Some(Line {
        kind: Kind::Carry { id: r.id.clone() },
        text: format!(
            "fael: {file} broke before — {id} \"{}\" was fixed: {} · whole story: `fael find {id}`\n",
            r.display_title(),
            clip(fix)
        ),
    })
}

/// `text` on one line, cut at `CLOSE_CHARS` with `…`.
fn clip(text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(CLOSE_CHARS) {
        Some((i, _)) => format!("{} …", one[..i].trim_end()),
        None => one,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The taught close puts its lesson (`guard` or `don't`) before `tried`,
    /// so a long close loses what failed to the clip, never the lesson.
    #[test]
    fn clip_keeps_the_taught_lesson() {
        let text = core::stats::CLOSE_TEMPLATE
            .replace("<cause>", "copy-as-new drops line quantity on every draft")
            .replace("<fix>", "copy every line field")
            .replace("guard `<test path>` or ", "")
            .replace("<X>", "copy fields by hand")
            .replace("<Y>", "copyLine() is the one list of fields")
            .replace(
                "<what failed>",
                "a per-field patch, then a deep clone that also copied ids",
            )
            .replace("[; (#N)]", "; (#326)");
        assert!(text.chars().count() > CLOSE_CHARS, "{text}");
        assert!(
            clip(&text).contains("copyLine() is the one list"),
            "{}",
            clip(&text)
        );
    }

    fn row(id: &str, kind: &str, text: &str) -> core::Row {
        core::Row {
            id: id.into(),
            ts: "2026-10-01T00:00:00Z".into(),
            kind: kind.into(),
            text: text.into(),
            files: vec!["src/a.rs".into()],
            ..core::Row::default()
        }
    }

    fn close(of: &str, text: &str) -> core::Row {
        core::Row {
            id: format!("01C{of}"),
            ts: "2026-10-02T00:00:00Z".into(),
            text: text.into(),
            reference: Some(of.into()),
            ..core::Row::default()
        }
    }

    fn said(log: &core::Log, skip: Option<&str>) -> Option<String> {
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
        carry_line(&ask, true, skip).map(|l| l.text)
    }

    #[test]
    fn a_closed_issue_whose_close_names_its_fix_is_carried_back() {
        let mut log = core::Log::default();
        log.rows
            .push(row("01AAAAAAAA", "issue", "parser drops the tail"));
        log.closes.push(close(
            "01AAAAAAAA",
            "off-by-one → slice to len; fixed in e6deb61",
        ));
        let got = said(&log, None).unwrap();
        assert!(
            got.contains("\"parser drops the tail\" was fixed: off-by-one"),
            "{got}"
        );
        assert!(got.contains("`fael find "), "{got}");
        // the gone-check line already names it: nothing twice
        assert!(said(&log, Some("01AAAAAAAA")).is_none());
    }

    #[test]
    fn no_fix_named_open_superseded_or_not_an_issue_stays_silent() {
        let mut log = core::Log::default();
        log.rows.push(row("01BBBBBBBB", "issue", "flaky"));
        log.closes
            .push(close("01BBBBBBBB", "not a bug, works as meant"));
        log.rows.push(row("01CCCCCCCC", "issue", "still open"));
        log.rows.push(row("01DDDDDDDD", "decision", "chose x"));
        log.closes.push(close("01DDDDDDDD", "done in (#12)"));
        assert!(said(&log, None).is_none());
        log.rows.push(row("01EEEEEEEE", "issue", "old fix"));
        log.closes.push(close("01EEEEEEEE", "fixed in (#40)"));
        let mut again = row("01FFFFFFFF", "issue", "came back");
        again.supersedes = Some("01EEEEEEEE".into());
        log.rows.push(again);
        assert!(said(&log, None).is_none());
    }

    /// `fael compact` folds a close into its row (`closed.text`): an old fix
    /// is carried back the same, and an old close naming none stays silent.
    #[test]
    fn a_close_folded_by_compact_is_carried_back_too() {
        let folded = |id: &str, text: &str| {
            let mut r = row(id, "issue", "cache never expires");
            let c = serde_json::json!({"id": "01CX", "ts": "2026-10-02T00:00:00Z", "by": "w", "text": text});
            r.extra.insert("closed".into(), c);
            r
        };
        let mut log = core::Log::default();
        log.rows.push(folded("01GGGGGGGG", "won't fix, by design"));
        assert!(said(&log, None).is_none());
        log.rows
            .push(folded("01HHHHHHHH", "no ttl → ttl 60s; guard (#7)"));
        let got = said(&log, None).unwrap();
        assert!(
            got.contains("was fixed: no ttl → ttl 60s; guard (#7)"),
            "{got}"
        );
    }

    /// PLAN-fael-experience-loop §4: the push path stays ≤ 5 ms. A hub file of
    /// 5000 closed issues in a 10000-row log, in a debug build, is held to ten
    /// times that, like `check.rs` — the worst silent case (the newest `SCAN`
    /// all closed naming no fix, each close looked up) and the said case.
    /// Best of 5, so a cold run on a shared CI runner is no fail (01M4FDXY).
    #[test]
    fn a_hub_of_closed_issues_stays_inside_the_push_budget() {
        let mut log = core::Log::default();
        for i in 0..5000 {
            let id = format!("01HUB{i:08}");
            log.rows.push(row(&id, "issue", &format!("problem {i}")));
            log.closes.push(close(&id, "works as meant"));
            let mut other = row(&format!("01OTH{i:08}"), "note", "x");
            other.files = vec![format!("src/other{i}.rs")];
            log.rows.push(other);
        }
        let timed = |log: &core::Log| {
            let run = || {
                let t = std::time::Instant::now();
                (said(log, None), t.elapsed())
            };
            let (got, d) = (0..5).map(|_| run()).min_by_key(|(_, d)| *d).unwrap();
            (got, d.as_secs_f64() * 1000.0)
        };
        let (got, silent) = timed(&log);
        assert!(got.is_none());
        // `(#N)`, not a sha: the hub scan is what grows with the log; a sha's
        // git lookup (`reached`) is a constant, bounded by `REACHED`
        log.closes.last_mut().unwrap().text = "cap → 3; fixed in (#61)".into();
        let (got, ms) = timed(&log);
        assert!(got.is_some());
        assert!(silent < 50.0 && ms < 50.0, "{silent:.1} ms / {ms:.1} ms");
    }

    #[test]
    fn a_long_close_is_clipped() {
        assert_eq!(clip("a  b\nc"), "a b c");
        let long = "x".repeat(CLOSE_CHARS + 5);
        assert!(clip(&long).ends_with(" …"));
        assert_eq!(clip(&long).chars().count(), CLOSE_CHARS + 2);
    }
}
