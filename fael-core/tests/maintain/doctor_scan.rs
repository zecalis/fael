use crate::common::*;
use fael_core::*;

#[test]
fn clean_log_passes() {
    let r = root();
    let fael = fael_of(&r);
    write_lines(
        &month_file(&fael, "tester-0000", "2026-07", false),
        &[row("A0000000000000000000000001", "note", &["a.rs"]).to_line()],
    );
    let rep = doctor_scan(&fael, &r, false, MONTH);
    assert!(rep.problems.is_empty(), "{rep:?}");
}

#[test]
fn no_log_is_info_not_error() {
    let r = root();
    let rep = doctor_scan(&fael_of(&r), &r, false, MONTH);
    assert_eq!(kinds(&rep), vec![(ProblemKind::NoLog, Severity::Info)]);
    assert_eq!(rep.errors().count(), 0);
}

#[test]
fn machine_readable_ids_ride_on_the_problem() {
    let r = root();
    let fael = fael_of(&r);
    let line = row("A0000000000000000000000001", "note", &["a.rs"]).to_line();
    write_lines(
        &month_file(&fael, "tester-0000", "2026-07", false),
        std::slice::from_ref(&line),
    );
    write_lines(&month_file(&fael, "other-0000", "2026-07", false), &[line]);
    let mut legacy = row("legacy-1a2b3c4d", "note", &[]);
    legacy.files = vec![];
    write_lines(
        &month_file(&fael, "legacy-0000", "2026-07", false),
        &[legacy.to_line()],
    );
    let rep = doctor_scan(&fael, &r, false, MONTH);
    // `--json` prints these: the full id, not the abbreviated example
    let d = rep
        .problems
        .iter()
        .find(|p| p.kind == ProblemKind::Duplicate)
        .unwrap();
    assert_eq!(d.ids, vec!["A0000000000000000000000001".to_string()]);
    let nf = rep
        .problems
        .iter()
        .find(|p| p.kind == ProblemKind::NoFiles)
        .unwrap();
    assert_eq!(nf.ids, vec!["legacy-1a2b3c4d".to_string()]);
}

#[test]
fn restore_row_is_no_problem() {
    // a restore event row is a carrier by design — doctor must not flag it
    // as NoFiles (or anything else)
    let r = root();
    let fael = fael_of(&r);
    let a = "A0000000000000000000000011";
    let b = "A0000000000000000000000012";
    let mut sup = row(b, "note", &["a.rs"]);
    sup.supersedes = Some(a.into());
    let mut back = Row::restored("tester-0000", b, a);
    back.ts = "2026-07-02T00:00:00.000Z".into();
    write_lines(
        &month_file(&fael, "tester-0000", "2026-07", false),
        &[
            row(a, "note", &["a.rs"]).to_line(),
            sup.to_line(),
            back.to_line(),
        ],
    );
    let rep = doctor_scan(&fael, &r, false, MONTH);
    assert!(rep.problems.is_empty(), "{rep:?}");
}

#[test]
fn missing_union_and_ignored() {
    let r = tmp(); // no .gitattributes here
    let fael = fael_of(&r);
    // nothing adopted: only the NoLog note, even with gitignore + no union line
    let rep = doctor_scan(&fael, &r, true, MONTH);
    assert_eq!(kinds(&rep), vec![(ProblemKind::NoLog, Severity::Info)]);
    // adopted (any log file): union + ignored fire
    write_lines(
        &month_file(&fael, "tester-0000", "2026-07", false),
        &[row("A0000000000000000000000001", "note", &["a.rs"]).to_line()],
    );
    let rep = doctor_scan(&fael, &r, true, MONTH);
    let ks = kinds(&rep);
    assert!(
        ks.contains(&(ProblemKind::Union, Severity::Error)),
        "{ks:?}"
    );
    assert!(
        ks.contains(&(ProblemKind::Ignored, Severity::Error)),
        "{ks:?}"
    );
}
