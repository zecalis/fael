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

#[test]
fn secret_row_is_an_unfixable_error_that_never_echoes_the_token() {
    let r = root();
    let fael = fael_of(&r);
    let token = format!("ghp_{}", "a".repeat(24));
    let mut leaked = row("A0000000000000000000000001", "note", &["a.rs"]);
    leaked.text = format!("token {token} pasted");
    let clean = row("A0000000000000000000000002", "note", &["b.rs"]);
    // the leaked row lives in a `.close` file too: a closed row still holds the bytes
    let mut closed = row("A0000000000000000000000003", "note", &["c.rs"]);
    closed.text = format!("old {token}");
    write_lines(
        &month_file(&fael, "tester-0000", "2026-07", false),
        &[leaked.to_line(), clean.to_line()],
    );
    write_lines(
        &month_file(&fael, "tester-0000", "2026-07", true),
        &[closed.to_line()],
    );
    let rep = doctor_scan(&fael, &r, false, MONTH);
    let hits: Vec<&Problem> = rep
        .problems
        .iter()
        .filter(|p| p.kind == ProblemKind::Secret)
        .collect();
    assert_eq!(hits.len(), 2, "{rep:?}");
    for p in &hits {
        assert_eq!(p.severity, Severity::Error);
        assert!(!p.fixable);
        assert!(!p.detail.contains(&token), "{}", p.detail);
        assert!(p.detail.contains("GitHub token") && p.detail.contains("rotate"));
        assert!(p.detail.contains(&format!("fael purge {}", p.ids[0])));
    }
    let mut ids: Vec<&str> = hits.iter().map(|p| p.ids[0].as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        ["A0000000000000000000000001", "A0000000000000000000000003"]
    );
}
