//! `plan migrate` after the sections: cited spec sections become reads (own plan, another
//! plan, a sub-section, a bare § with one spec), the rest is reported; links to spec and plan
//! md name where the text lives; code scans are reported; a second run writes nothing.

use super::super::migrate::Spec;
use super::super::{Fields, Store};
use super::{cites, scan};

fn spec(f: &str, text: &str) -> Spec {
    Spec {
        app: String::new(),
        path: format!(".fapony/spec/{f}"),
        text: text.into(),
    }
}

fn specs() -> Vec<Spec> {
    vec![
        spec(
            "SPEC-access.md",
            "> **Used by:** [PLAN-a.md](../plan/PLAN-a.md)\n\n## §1 Roles\nr\n\n\
             ## §2 Flags\nsee [SPEC-b §1](SPEC-b.md#s1), [PLAN-b.md](../plan/PLAN-b.md), \
             [the spec](SPEC-access.md) and [docs](docs/x.md)\n\n## §17 Big\n### 17.4 Sub\nx\n\n\
             ## Code anchors (scan 2026-10-04)\na\nb\n\n\
             ## <a id=\"t\"></a>src/server/lib/tax/ (root files)\nfn x\n",
        ),
        spec("SPEC-b.md", "> **Used by:** PLAN-b\n## §1 Rule\nb\n"),
        spec("SPEC-c.md", "> **Used by:** PLAN-b\n## §1 Other\nc\n"),
    ]
}

fn add(s: &mut Store, plan: &str, title: &str, brief: &str, reads: &[&str]) -> String {
    let f = Fields {
        title: Some(title.into()),
        brief: Some(brief.into()),
        reads: Some(reads.iter().map(|r| r.to_string()).collect()),
        ..Fields::default()
    };
    s.add(plan, &f, &[], "t0").unwrap()
}

fn reads(s: &Store, uid: &str) -> String {
    s.conn
        .query_row(
            "SELECT COALESCE(reads, '') FROM chunk WHERE uid = ?1",
            [uid],
            |r| r.get(0),
        )
        .unwrap()
}

fn body(s: &Store, key: &str) -> String {
    s.conn
        .query_row("SELECT body FROM section WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .unwrap()
}

#[test]
fn cited_sections_become_reads_and_links_name_their_new_home() {
    let mut s = Store::open_in_memory().unwrap();
    for p in ["a", "b", "c"] {
        s.new_plan("", p, p, "dev", "t0").unwrap();
    }
    let own = add(&mut s, "a", "do SPEC-access §1 then §2", "", &[]);
    let other = add(
        &mut s,
        "a",
        "rules",
        "per SPEC-b.md §1 and SPEC-access §1",
        &[],
    );
    let sub = add(&mut s, "a", "SPEC-access §17.4 and SPEC-access §9", "", &[]);
    let two = add(&mut s, "b", "fix §1", "", &[]);
    let none = add(&mut s, "c", "see §3", "", &[]);
    let kept = add(&mut s, "a", "SPEC-access §1", "", &["1"]);

    let out = s.migrate_md(&|_| None, &specs(), false).unwrap();
    assert_eq!(reads(&s, &own), "access:§1\naccess:§2");
    assert_eq!(reads(&s, &other), "b/b:§1\naccess:§1");
    assert_eq!(reads(&s, &sub), "access:§17", "§17.4 sits in §17");
    assert!(
        out.contains(&format!("missing: {sub} SPEC-access §9")),
        "{out}"
    );
    assert_eq!(reads(&s, &two), "");
    assert!(
        out.contains(&format!("ambiguous: {two} §1 — its plan holds 2 specs")),
        "{out}"
    );
    assert!(
        out.contains(&format!("ambiguous: {none} §3 — its plan holds 0 specs")),
        "{out}"
    );
    assert_eq!(reads(&s, &kept), "1", "a chunk with reads is left alone");

    assert_eq!(
        body(&s, "access:§2"),
        "see SPEC-b §1 (`b/b:§1`), plan `b`, the spec (spec `access`, plan `a`) and [docs](docs/x.md)"
    );
    assert_eq!(body(&s, "access:intro"), "> **Used by:** plan `a`");
    assert!(
        out.contains("code-scan: a access:code-anchors-scan-2026-10-04 (2 lines)"),
        "{out}"
    );
    assert!(
        out.contains("3 chunks given reads (2 ambiguous, 1 missing), 2 sections relinked, 2 code-scan sections"),
        "{out}"
    );

    let again = s.migrate_md(&|_| None, &specs(), false).unwrap();
    assert!(again.ends_with("nothing to write"), "{again}");
    assert!(again.contains("0 chunks given reads"), "{again}");
    assert_eq!(reads(&s, &own), "access:§1\naccess:§2");
}

#[test]
fn cites_and_scans_read_only_what_is_written() {
    assert_eq!(
        cites("SPEC-vela-access §13, SPEC-x.md §3.1. and §B/§D"),
        [
            (Some("vela-access".into()), "13".into()),
            (Some("x".into()), "3.1".into()),
            (None, "B".into()),
            (None, "D".into()),
        ]
    );
    assert!(cites("§ alone").is_empty());
    assert_eq!(
        cites("title §1\nbrief §1").len(),
        1,
        "a cite in title and brief once"
    );
    assert!(scan("Appendix — code moved in") && scan("<a id=\"x\"></a>src/a/b (root files)"));
    assert!(!scan("§6 Nav") && !scan("A/B tests"));
}
