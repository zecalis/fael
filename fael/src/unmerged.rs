//! A close that cites a commit not on the default branch (issue 01M4Q6WE6P): if that
//! branch is dropped, the issue stays closed with no guard behind it. Warn, never reject.

use std::path::Path;
use std::process::Command;

/// One `warning:` line per commit cited in `text` that git knows and the remote's
/// default branch does not contain. No remote ref, or a hex word git can't resolve → silent.
pub(crate) fn lines(root: &Path, text: &str) -> Vec<String> {
    let base = format!("origin/{}", crate::maintain::default_branch(root));
    if crate::git(root, &["rev-parse", "--verify", "-q", &base]).is_none() {
        return vec![];
    }
    let mut out = vec![];
    for tok in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        // ponytail: a sha needs a digit and a-f letter, so plain words and numbers never spawn git
        let sha = (7..=40).contains(&tok.len())
            && tok
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && tok.bytes().any(|b| b.is_ascii_digit())
            && tok.bytes().any(|b| b.is_ascii_alphabetic());
        if !sha || out.iter().any(|l: &String| l.contains(tok)) {
            continue;
        }
        let commit = format!("{tok}^{{commit}}");
        if crate::git(root, &["rev-parse", "--verify", "-q", &commit]).is_none() {
            continue;
        }
        let on_base = Command::new("git")
            .args(["merge-base", "--is-ancestor", tok, &base])
            .current_dir(root)
            .status()
            .is_ok_and(|s| s.success());
        if !on_base {
            out.push(format!(
                "warning: commit {tok} is not on {base} — its guard holds only once it merges; if that branch is dropped, reopen the issue"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let o = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn commit(dir: &Path, msg: &str) -> String {
        git(
            dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                msg,
            ],
        );
        git(dir, &["rev-parse", "HEAD"])
    }

    #[test]
    fn warns_only_for_a_commit_off_the_default_branch() {
        let dir = std::env::temp_dir().join(format!("fael-unmerged-{}", fael_core::ulid()));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        let on = commit(&dir, "on main");
        let quiet = lines(&dir, &format!("guard in {on}"));
        assert!(quiet.is_empty(), "no origin ref yet → silent: {quiet:?}");
        git(&dir, &["update-ref", "refs/remotes/origin/main", &on]);
        let off = commit(&dir, "on a branch");
        let why = format!("fixed in {on} and {off}; guard x.rs (b7552a3, 1234567, deadbeef)");
        let got = lines(&dir, &why);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(
            got[0].starts_with(&format!("warning: commit {off} is not on origin/main")),
            "{got:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
