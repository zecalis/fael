//! `chunk start`: the commits on the default branch since the brief was written that touch
//! the chunk's scope (`Brief::landed`). A run's drift (`chunk status`) starts at its base,
//! so at start it is empty even when the brief predates weeks of work on its files — this
//! fills that window. One `git log` per chunk; offline (`origin/HEAD` as last fetched).

use crate::Repo;
use fael_crew::Started;

/// Most commits a brief names; the rest ride a count.
const MAX: usize = 5;

pub(crate) fn resolve(r: &Repo, st: &mut Started) {
    let Some(main) = crate::git(&r.root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]) else {
        return;
    };
    for c in st.chunks.iter_mut() {
        let (Some(since), Some(scope)) = (c.started.as_deref(), c.scope.as_deref()) else {
            continue;
        };
        let paths = paths(scope);
        if paths.is_empty() {
            continue;
        }
        let all = since_brief(r, &main, since, &paths);
        c.landed_more = all.len().saturating_sub(MAX);
        c.landed = all.into_iter().take(MAX).collect();
    }
}

/// Scope lines as pathspecs: a trailing `— leave alone`/comment after the path is dropped.
fn paths(scope: &str) -> Vec<String> {
    scope
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

/// `<short sha> <subject>`, newest first; empty when git cannot say.
fn since_brief(r: &Repo, main: &str, since: &str, paths: &[String]) -> Vec<String> {
    let since = format!("--since={since}");
    let mut args = vec!["log", "--format=%h %s", since.as_str(), main, "--"];
    args.extend(paths.iter().map(String::as_str));
    crate::git(&r.root, &args)
        .map(|out| out.lines().map(String::from).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::paths;

    #[test]
    fn scope_lines_become_pathspecs() {
        assert_eq!(
            paths("fael/src/hook/\nREADME.md — leave alone\n\n"),
            vec!["fael/src/hook/", "README.md"]
        );
    }
}
