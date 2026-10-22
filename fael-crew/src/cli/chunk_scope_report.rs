//! A brief that cites an open row filed `from: report` (a claim relayed from someone else's
//! report, never checked against the code — issue 01M5H3FGE): a warning line per row so the
//! agent confirms its file:line first. A warning only, never a refusal.

use std::collections::HashSet;

/// One line per open `from: report` row the title or brief names by id token.
pub(crate) fn warnings(title: &str, brief: &str, log: &fael_core::Log) -> Vec<String> {
    let shut = fael_core::closed(log);
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for tok in fael_core::id_tokens(&format!("{title} {brief}")) {
        let fael_core::Ref::One(row) = fael_core::ref_state(log, tok) else {
            continue;
        };
        if row.from_report() && !shut.contains(row.id.as_str()) && seen.insert(row.id.as_str()) {
            out.push(format!(
                "warning: {tok} is from a report, unchecked — confirm its file:line before building on it"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::warnings;

    fn row(id: &str, from: Option<&str>) -> fael_core::Row {
        fael_core::Row {
            id: id.into(),
            kind: "issue".into(),
            text: "x".into(),
            from: from.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn an_open_report_row_warns_and_others_stay_silent() {
        let (a, b, c) = (
            "01M0000000000000000000001",
            "01M0000000000000000000002",
            "01M0000000000000000000003",
        );
        let mut closed = fael_core::Row::new("x", "note", "closed it", vec![]);
        closed.reference = Some(c.into());
        let log = fael_core::Log {
            rows: vec![
                row(a, Some("report")),
                row(b, Some("user")),
                row(c, Some("report")),
            ],
            closes: vec![closed],
            ..Default::default()
        };
        let w = warnings("t", &format!("fix {a}, {b} and {c}"), &log);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains(a) && w[0].contains("from a report, unchecked"),
            "{w:?}"
        );
    }
}
