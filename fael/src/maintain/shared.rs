//! `[Shared]` (PLAN-fael-parallel shared-files): every `.fael/shared` path is merged by
//! git, never claimed — so each one says how it merges. A path with no `merge` attribute
//! gets the `.gitattributes` line to add; doctor never writes it (append-only → union,
//! anything else → text; generated files are re-blessed after a rebase, never hand-merged).

use crate::core;
use std::path::Path;

pub(super) fn notes(root: &Path) -> Vec<core::Problem> {
    let note = |s: String| core::Problem::info(core::ProblemKind::Shared, s);
    let shared = match core::work::shared(root) {
        Ok(s) if s.is_empty() => return Vec::new(),
        Ok(s) => s,
        Err(e) => return vec![note(e)],
    };
    // a directory prefix is checked as a file under it: `docs/**` covers `docs/x`
    let probe = |p: &String| {
        if p.ends_with('/') {
            format!("{p}x")
        } else {
            p.clone()
        }
    };
    let mut args = vec!["check-attr".to_string(), "merge".into(), "--".into()];
    args.extend(shared.iter().map(probe));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(out) = crate::git(root, &args) else {
        return Vec::new();
    };
    shared
        .iter()
        .zip(out.lines())
        .filter(|(_, l)| l.ends_with(": merge: unspecified"))
        .map(|(p, _)| {
            let pat = if p.ends_with('/') {
                format!("{p}**")
            } else {
                p.clone()
            };
            note(format!(
                "shared {p} has no merge attribute — append-only: `{pat} merge=union` in \
                 .gitattributes; else `{pat} merge=text` (generated: re-bless after a rebase)"
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_path_with_no_merge_attribute_gets_the_line() {
        let d = std::env::temp_dir().join(format!("fael-doctor-shared-{}", fael_core::ulid()));
        std::fs::create_dir_all(d.join(".fael")).unwrap();
        let mut init = std::process::Command::new("git");
        assert!(
            init.args(["init", "-q"])
                .current_dir(&d)
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(d.join(".fael/shared"), "a.md\ndocs/\ngold/\n").unwrap();
        std::fs::write(d.join(".gitattributes"), "docs/** merge=text\n").unwrap();
        let n: Vec<String> = super::notes(&d).into_iter().map(|p| p.detail).collect();
        assert_eq!(n.len(), 2, "{n:?}");
        assert!(n[0].contains("`a.md merge=union`"), "{n:?}");
        assert!(n[1].contains("`gold/** merge=text`"), "{n:?}");
        std::fs::write(d.join(".fael/shared"), "../x\n").unwrap();
        assert!(super::notes(&d)[0].detail.contains("rejected"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
