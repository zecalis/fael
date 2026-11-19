//! How `fael doctor` prints its report: the header says what is actionable,
//! the lines follow in the same order, and a kind that lists only a few
//! examples ends with the command that prints the rest. `--json` keeps its
//! shape (one object per problem, in detection order); only `detail` grows
//! by the entries the text line leaves out.

use crate::core;
use core::ProblemKind as K;

/// What a reader does with one problem line.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// Fails `doctor` (exit 1) and `--fix` cannot repair it.
    Error,
    /// A judgement only the reader can make: check it, then close, re-file or bump.
    Decide,
    /// `--fix` clears it now.
    Fix,
    /// Nothing to do for the row log: where rows live, a metric, a branch hint,
    /// a caveat, machine state. Listed last, never counted as a problem.
    Info,
}

/// Exhaustive on purpose: a new kind must say which it is.
fn class(p: &core::Problem) -> Class {
    if p.fixable {
        return Class::Fix;
    }
    if p.severity == core::Severity::Error {
        return Class::Error;
    }
    match p.kind {
        K::Local
        | K::Precision
        | K::Merged
        | K::GhOffline
        | K::Wiring
        | K::Shared
        | K::NoLog
        | K::MultiFael
        | K::Future
        | K::NoFiles
        | K::NoVerdict => Class::Info,
        K::Broken
        | K::Torn
        | K::Conflict
        | K::Duplicate
        | K::Encoding
        | K::Oversize
        | K::Gone
        | K::PartGone
        | K::Stale
        | K::Superseded
        | K::Phantom
        | K::Orphan
        | K::Fat
        | K::Shipped
        | K::ShippedMaybe
        | K::ShippedKept
        | K::NotEnglish
        | K::Drifted
        | K::Unstamped
        | K::Secret => Class::Decide,
    }
}

/// The command that prints every entry a kind line cut.
const REST: &str = "fael doctor --json";

pub(super) fn show(rep: &core::DoctorReport, json: bool) {
    if json {
        let ps: Vec<_> = rep
            .problems
            .iter()
            .map(|p| {
                serde_json::json!({
                    "kind": kind_label(&p.kind).to_lowercase(),
                    "severity": format!("{:?}", p.severity).to_lowercase(),
                    "fixable": p.fixable,
                    "ids": p.ids,
                    "detail": full_detail(p),
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(ps));
        return;
    }
    print!("{}", text(rep));
}

/// `--json` carries every entry: the text line's `+N more` lands here.
fn full_detail(p: &core::Problem) -> String {
    if p.more.is_empty() {
        p.detail.clone()
    } else {
        format!("{} · the rest: {}", p.detail, p.more.join("; "))
    }
}

/// The text report: header, then actionable lines before info lines.
fn text(rep: &core::DoctorReport) -> String {
    if rep.problems.is_empty() {
        return "fael doctor: clean\n".into();
    }
    let mut ps: Vec<(Class, &core::Problem)> = rep.problems.iter().map(|p| (class(p), p)).collect();
    ps.sort_by_key(|(c, _)| *c); // stable: detection order inside a class
    let n = |c: Class| ps.iter().filter(|(k, _)| *k == c).count();
    let mut parts = vec![];
    for (c, what) in [
        (Class::Error, "error(s)"),
        (Class::Decide, "need a decision"),
        (Class::Fix, "--fix clears now"),
    ] {
        if n(c) > 0 {
            parts.push(format!("{} {what}", n(c)));
        }
    }
    if parts.is_empty() {
        parts.push("nothing to decide".into());
    }
    if n(Class::Info) > 0 {
        parts.push(format!("{} info", n(Class::Info)));
    }
    let mut out = format!("fael doctor: {}\n", parts.join(" · "));
    for (c, p) in ps {
        let sev = match c {
            Class::Error => "error",
            Class::Info => "info",
            Class::Decide | Class::Fix => "note",
        };
        let fix = if p.fixable { " [--fix]" } else { "" };
        let rest = if p.more.is_empty() {
            String::new()
        } else {
            format!(" · +{} more: `{REST}`", p.more.len())
        };
        out.push_str(&format!(
            "{sev} [{}]{fix}: {}{rest}\n",
            kind_label(&p.kind),
            p.detail
        ));
    }
    out
}

/// The `[Shipped?]` / `[Shipped kept]` labels for the two Shipped variants —
/// every other kind renders as its debug name.
pub(super) fn kind_label(k: &core::ProblemKind) -> String {
    match k {
        K::ShippedMaybe => "Shipped?".into(),
        K::ShippedKept => "Shipped kept".into(),
        _ => format!("{k:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(ps: Vec<core::Problem>) -> core::DoctorReport {
        core::DoctorReport { problems: ps }
    }

    #[test]
    fn the_header_counts_what_is_actionable_and_info_goes_last() {
        let fixable =
            core::Problem::info(K::Gone, "g".into()).with_closes(vec![("i".into(), "gone".into())]);
        let out = text(&rep(vec![
            core::Problem::info(K::Local, "l".into()),
            core::Problem::info(K::PartGone, "p".into()),
            core::Problem::info(K::Drifted, "d".into()),
            fixable,
            core::Problem::info(K::Merged, "m".into()),
        ]));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines[0],
            "fael doctor: 2 need a decision · 1 --fix clears now · 2 info"
        );
        let tags: Vec<&str> = lines[1..]
            .iter()
            .map(|l| l.split(':').next().unwrap())
            .collect();
        assert_eq!(
            tags,
            [
                "note [PartGone]",
                "note [Drifted]",
                "note [Gone] [--fix]",
                "info [Local]",
                "info [Merged]"
            ]
        );
    }

    #[test]
    fn only_info_reads_as_nothing_to_decide_and_no_problems_as_clean() {
        let out = text(&rep(vec![core::Problem::info(K::Local, "l".into())]));
        assert!(
            out.starts_with("fael doctor: nothing to decide · 1 info\n"),
            "{out}"
        );
        assert_eq!(text(&rep(vec![])), "fael doctor: clean\n");
    }

    #[test]
    fn a_cut_list_names_the_command_and_json_carries_the_rest() {
        let p = core::Problem::info(K::Drifted, "7 rows (e.g. a)".into())
            .with_more(vec!["b".into(), "c".into()]);
        let out = text(&rep(vec![p.clone()]));
        assert!(
            out.contains("(e.g. a) · +2 more: `fael doctor --json`"),
            "{out}"
        );
        assert_eq!(full_detail(&p), "7 rows (e.g. a) · the rest: b; c");
        let whole = core::Problem::info(K::Drifted, "2 rows (e.g. a; b)".into());
        assert!(!text(&rep(vec![whole])).contains("more"));
    }
}
