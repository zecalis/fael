//! A close that cites a commit not on the default branch (issue 01M4Q6WE6P): if that
//! branch is dropped, the issue stays closed with no guard behind it. Warn, never reject.

use std::path::Path;

/// One `warning:` line per commit cited in `text` that git knows and the remote's
/// default branch does not contain. No remote ref, or a hex word git can't resolve → silent.
/// Fixed spawns however many shas `text` cites (issue 01M5F53Y): one
/// `cat-file --batch-check` for existence plus one `rev-list` for ancestry.
pub(crate) fn lines(root: &Path, text: &str) -> Vec<String> {
    let base = format!("origin/{}", crate::maintain::default_branch(root));
    if crate::git(root, &["rev-parse", "--verify", "-q", &base]).is_none() {
        return vec![];
    }
    let mut toks: Vec<String> = vec![];
    for tok in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        // ponytail: a sha needs a digit and a-f letter, so plain words and numbers never spawn git
        let sha = (7..=40).contains(&tok.len())
            && tok
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && tok.bytes().any(|b| b.is_ascii_digit())
            && tok.bytes().any(|b| b.is_ascii_alphabetic());
        if !sha || toks.iter().any(|t| t == tok) {
            continue;
        }
        toks.push(tok.to_string());
    }
    if toks.is_empty() {
        return vec![];
    }
    let known = existing(root, &toks);
    let ancestors = ancestors_of(root, &base);
    let mut out = vec![];
    for tok in toks {
        if !known.contains(&tok) || ancestors.contains(&tok) {
            continue;
        }
        out.push(format!(
            "warning: commit {tok} is not on {base} — its guard holds only once it merges; if that branch is dropped, reopen the issue"
        ));
    }
    out
}

/// The subset of `toks` git knows as commits, in one `cat-file --batch-check`.
fn existing(root: &Path, toks: &[String]) -> std::collections::HashSet<String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut known = std::collections::HashSet::new();
    let Ok(mut child) = fael_core::git_cmd()
        .args(["cat-file", "--batch-check"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return known;
    };
    let input: String = toks.iter().map(|s| format!("{s}^{{commit}}\n")).collect();
    let mut w = child.stdin.take();
    let out = std::thread::scope(|sc| {
        sc.spawn(move || {
            if let Some(w) = w.as_mut() {
                let _ = w.write_all(input.as_bytes());
            }
        });
        child.wait_with_output()
    });
    let Ok(out) = out else { return known };
    for (line, tok) in String::from_utf8_lossy(&out.stdout).lines().zip(toks) {
        if line
            .split_whitespace()
            .nth(1)
            .is_some_and(|t| t == "commit")
        {
            known.insert(tok.clone());
        }
    }
    known
}

/// Full shas on `base`, in one `rev-list`; empty (never `None`) when git fails.
fn ancestors_of(root: &Path, base: &str) -> std::collections::HashSet<String> {
    crate::git(root, &["rev-list", base])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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
