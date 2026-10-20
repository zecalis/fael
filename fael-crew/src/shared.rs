//! `.fael/shared` (PLAN-fael-parallel shared-files): the paths every chunk touches —
//! registries, docs, goldens. A shared path is never a claim: overlaps skip it, and
//! `git merge-tree` stays its guard. One repo-relative path prefix per line, scope's
//! rules; blank lines and `#` comments skipped; no file = none.

use super::board_math::under;
use super::chunk::scope;
use std::path::Path;

/// The repo's shared prefixes; a line scope would reject is an error naming the file.
pub fn shared(root: &Path) -> Result<Vec<String>, String> {
    let path = root.join(".fael/shared");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect();
    scope(&lines).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(lines)
}

/// A scope less every path a shared prefix covers (`docs/` stays when only
/// `docs/architecture.md` is shared: the rest of `docs/` is still the chunk's).
pub(super) fn unshared(scope: &[String], shared: &[String]) -> Vec<String> {
    scope
        .iter()
        .filter(|p| !shared.iter().any(|s| under(p, s)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_prefix_drops_only_what_it_covers() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let shared = s(&["docs/architecture.md", "fael/tests/golden/"]);
        assert_eq!(
            unshared(
                &s(&[
                    "docs/",
                    "docs/architecture.md",
                    "fael/tests/golden/b.json",
                    "src/a.rs"
                ]),
                &shared
            ),
            s(&["docs/", "src/a.rs"])
        );
    }

    #[test]
    fn the_file_is_read_by_scope_rules() {
        let d = std::env::temp_dir().join(format!("fael-shared-{}", std::process::id()));
        std::fs::create_dir_all(d.join(".fael")).unwrap();
        assert_eq!(shared(&d).unwrap(), Vec::<String>::new(), "no file = none");
        std::fs::write(
            d.join(".fael/shared"),
            "# registries\nAGENTS.md\n\n docs/ \n",
        )
        .unwrap();
        assert_eq!(shared(&d).unwrap(), ["AGENTS.md", "docs/"]);
        std::fs::write(d.join(".fael/shared"), "src/*.rs\n").unwrap();
        assert!(shared(&d).unwrap_err().contains(".fael/shared"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
