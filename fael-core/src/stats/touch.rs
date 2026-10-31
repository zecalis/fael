//! What a session did with a row it was handed (issue 01M5ZGYRQ): cited its
//! id (`outcome` line), found it by id (`find <id>`), or read/edited one of
//! its files (any usage line that names `files`). An in-context line exists
//! only when an edit lands, so counting just that scored a read-only session
//! a miss on every row. Union with it, never instead: a lower bound still,
//! only less low. Pure: kept usage rows in.

use super::parse::{Parsed, session_stem};
use crate::Log;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Did<'a> {
    ids: HashSet<&'a str>,
    files: HashSet<&'a str>,
}

/// Per (repo, session): the ids it cited or found and the files it touched.
pub(super) struct Touches<'a>(HashMap<(&'a str, &'a str), Did<'a>>);

fn strs<'a>(v: &'a serde_json::Value, k: &str) -> impl Iterator<Item = &'a str> {
    v[k].as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i.as_str())
}

impl<'a> Touches<'a> {
    pub(super) fn index(parsed: &'a Parsed) -> Self {
        let mut out: HashMap<(&str, &str), Did> = HashMap::new();
        for v in &parsed.kept {
            let (Some(repo), Some(s)) = (v["repo"].as_str(), v["session"].as_str()) else {
                continue;
            };
            let did = out.entry((repo, session_stem(s))).or_default();
            did.ids.extend(strs(v, "cited"));
            did.ids.extend(v["q"]["id"].as_str());
            did.files.extend(strs(v, "files"));
        }
        Touches(out)
    }

    /// Whether the session cited or found `id`, or touched one of `files`.
    pub(super) fn acted(&self, repo: &str, session: &str, id: &str, log: Option<&Log>) -> bool {
        let Some(did) = self.0.get(&(repo, session_stem(session))) else {
            return false;
        };
        did.ids.contains(id)
            || log
                .and_then(|l| l.rows.iter().find(|r| r.id == id))
                .is_some_and(|r| r.files.iter().any(|f| did.files.contains(f.as_str())))
    }
}
