//! A path a chunk names that is not on disk but that git history shows deleted or renamed
//! (issue 01M5GZ3HA): said once, so the agent does not guess the successor. A path git never
//! had stays silent — the chunk may create it. A warning only, never a refusal.

use std::path::Path;

/// The warning for `p` named by chunk `uid`, or `None` when it exists or git never had it.
/// `at_start`: the agent starting the chunk reads it, so it never offers `chunk edit`.
pub(crate) fn warning(root: &Path, uid: &str, p: &str, at_start: bool) -> Option<String> {
    let p = p.trim_end_matches('/');
    if p.is_empty() || root.join(p).exists() {
        return None;
    }
    // a pathspec hides the rename's other side, so find the commit by path, then read it whole
    let sha = crate::git(
        root,
        &["log", "-1", "--diff-filter=DR", "--format=%h", "--", p],
    )?;
    let changes = crate::git(root, &["show", "-M", "--name-status", "--format=", &sha])?;
    let (what, now) = changes.lines().find_map(|l| {
        let mut f = l.split('\t');
        match (f.next()?, f.next()?, f.next()) {
            ("D", old, _) if old == p => {
                Some(("removed".to_string(), "what replaced it".to_string()))
            }
            (s, old, Some(new)) if s.starts_with('R') && old == p => {
                Some((format!("renamed to {new}"), new.to_string()))
            }
            _ => None,
        }
    })?;
    let ask = if at_start {
        format!(
            "the brief is older than the code: work from {now}; tell the owner in the done text"
        )
    } else {
        format!("fael chunk edit {uid} --brief … (or --scope …)")
    };
    Some(format!(
        "warning: {p} named by chunk {uid} was {what} in {sha} — {ask}"
    ))
}

#[cfg(test)]
mod tests {
    use super::warning;
    use std::path::Path;
    use std::process::Command;

    fn git(d: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(d)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn removed_and_renamed_paths_warn_and_a_never_seen_path_stays_silent() {
        let d = std::env::temp_dir().join(format!("fael-gone-{}", fael_core::ulid()));
        std::fs::create_dir_all(d.join("e2e")).unwrap();
        git(&d, &["init", "-q", "-b", "main"]);
        std::fs::write(d.join("e2e/gone.ts"), "export const gone = 1;\n".repeat(5)).unwrap();
        std::fs::write(d.join("e2e/old.ts"), "export const old = 2;\n".repeat(5)).unwrap();
        std::fs::write(d.join("e2e/kept.ts"), "k\n").unwrap();
        git(&d, &["add", "."]);
        git(&d, &["commit", "-qm", "init"]);
        git(&d, &["rm", "-q", "e2e/gone.ts"]);
        git(&d, &["mv", "e2e/old.ts", "e2e/new.ts"]);
        git(&d, &["commit", "-qm", "drop and move"]);
        let sha = crate::git(&d, &["rev-parse", "--short", "HEAD"]).unwrap();

        let rm = warning(&d, "W1", "e2e/gone.ts", false).unwrap();
        assert!(rm.contains("removed in") && rm.contains(&sha), "{rm}");
        let mv = warning(&d, "W1", "e2e/old.ts", false).unwrap();
        assert!(
            mv.contains("renamed to e2e/new.ts in") && mv.contains(&sha),
            "{mv}"
        );
        assert!(mv.contains("fael chunk edit W1"), "{mv}");
        let st = warning(&d, "W1", "e2e/old.ts", true).unwrap();
        assert!(
            st.contains("work from e2e/new.ts") && !st.contains("chunk edit"),
            "{st}"
        );
        let st = warning(&d, "W1", "e2e/gone.ts", true).unwrap();
        assert!(st.contains("work from what replaced it"), "{st}");
        assert_eq!(warning(&d, "W1", "e2e/never.ts", false), None);
        assert_eq!(warning(&d, "W1", "e2e/kept.ts", false), None);
        std::fs::remove_dir_all(&d).ok();
    }
}
