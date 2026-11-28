//! Which unlinked open issues could ride one fix: `fits` = open chunks whose scope names one
//! of the issue's files (link it there: `fael chunk edit <uid> --issue +<id>`), `shares` = other
//! unlinked issues on the same files. `fael sweep --input` and the board's ledger carry them
//! so a sweep or the owner groups without re-reading every scope. Facts only: two issues on
//! one file may still have unrelated causes — grouping is the reader's call.
//! Same file only: a scope is mostly directories, and a dir covering a file matched most
//! open issues on this machine's board (34 lines on 2026-11-29) — noise, not a group.

/// The uids of `open` (`(uid, scope)`) whose scope overlaps `files`, in `open` order.
pub(crate) fn fits(files: &[String], open: &[(String, Vec<String>)]) -> Vec<String> {
    if files.is_empty() {
        return vec![];
    }
    open.iter()
        .filter(|(_, scope)| same_file(files, scope))
        .map(|(uid, _)| uid.clone())
        .collect()
}

/// The ids of `others` (`(id, files)`) other than `id` whose files overlap `files`.
pub(crate) fn shares(id: &str, files: &[String], others: &[(String, Vec<String>)]) -> Vec<String> {
    if files.is_empty() {
        return vec![];
    }
    others
        .iter()
        .filter(|(o, f)| o != id && same_file(files, f))
        .map(|(o, _)| o.clone())
        .collect()
}

fn same_file(a: &[String], b: &[String]) -> bool {
    fael_core::overlap_class(a, b) == Some(fael_core::OverlapClass::Exact)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn only_a_scope_naming_the_same_file_fits_never_a_dir_covering_it() {
        let open = vec![
            ("C1".to_string(), v(&["fael/src/hook/"])),
            ("C2".to_string(), v(&["fael/src/hook/say.rs", "README.md"])),
        ];
        assert_eq!(fits(&v(&["fael/src/hook/say.rs"]), &open), v(&["C2"]));
        assert!(fits(&[], &open).is_empty());
    }

    #[test]
    fn shares_names_the_other_issues_on_the_file_never_itself() {
        let all = vec![
            ("A".to_string(), v(&["x.rs"])),
            ("B".to_string(), v(&["x.rs", "y.rs"])),
            ("C".to_string(), v(&["z.rs"])),
        ];
        assert_eq!(shares("A", &v(&["x.rs"]), &all), v(&["B"]));
        assert!(shares("C", &v(&["z.rs"]), &all).is_empty());
    }
}
