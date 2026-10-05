//! groups — open rows linked by a shared file, transitively; docs and anchors
//! never link; largest group first, a lone row after.

use super::{ids, row};
use fael_core::*;

#[test]
fn shared_files_link_transitively() {
    let r = |id: &str, files: &[&str]| row(id, "issue", files, None);
    let rows = [
        r("A0000000000000000000000010", &["src/ocr.rs", "src/doc.rs"]),
        r("A0000000000000000000000011", &["src/auth.rs"]),
        r(
            "A0000000000000000000000012",
            &["src/doc.rs", "src/scope.rs"],
        ),
        r("A0000000000000000000000013", &["src/scope.rs"]),
        // a spec and an anchor every issue cites are context, not a shared edit
        r("A0000000000000000000000014", &["SPEC.md", "plan:vela"]),
        r(
            "A0000000000000000000000015",
            &["src/auth.rs", "SPEC.md", "plan:vela"],
        ),
    ];
    let refs: Vec<&Row> = rows.iter().collect();
    let g = groups(&refs);
    let g: Vec<Vec<String>> = g.iter().map(|g| ids(g)).collect();
    // 10-12 via doc.rs, 12-13 via scope.rs; 11-15 via auth.rs; 14 alone
    assert_eq!(g, [vec!["10", "12", "13"], vec!["11", "15"], vec!["14"]]);
}

#[test]
fn uppercase_md_never_links() {
    let r = |id: &str, files: &[&str]| row(id, "issue", files, None);
    let rows = [
        r("A0000000000000000000000010", &["SPEC.MD"]),
        r("A0000000000000000000000011", &["src/auth.rs", "SPEC.MD"]),
        r("A0000000000000000000000012", &["src/auth.rs"]),
    ];
    let refs: Vec<&Row> = rows.iter().collect();
    let g = groups(&refs);
    let g: Vec<Vec<String>> = g.iter().map(|g| ids(g)).collect();
    // 11-12 via auth.rs; 10 alone — SPEC.MD is prose, not a shared edit
    assert_eq!(g, [vec!["11", "12"], vec!["10"]]);
}

#[test]
fn render_names_the_shared_files() {
    let r = |id: &str, files: &[&str]| row(id, "issue", files, None);
    let rows = [
        r("A0000000000000000000000010", &["src/a.rs", "src/b.rs"]),
        r("A0000000000000000000000011", &["src/b.rs"]),
        r("A0000000000000000000000012", &["src/c.rs"]),
    ];
    let refs: Vec<&Row> = rows.iter().collect();
    let out = render_groups(&Log::default(), &refs);
    assert!(
        out.starts_with("## group 1 · 2 rows · shared: src/b.rs\n"),
        "{out}"
    );
    assert!(out.contains("## group 2 · shares no file\n"), "{out}");
    assert_eq!(
        out.lines().filter(|l| l.starts_with("- [")).count(),
        3,
        "{out}"
    );
}
