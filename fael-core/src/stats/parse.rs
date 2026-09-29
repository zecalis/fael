//! Parse `usage.jsonl` text into counts — pure: text in, struct out.
//!
//! Temp-dir repos (benchmarks, tests) are skipped unless the state file
//! itself is scratch (01M3CRR6A). The caller passes both sides in, so there
//! is no clock, env or filesystem read here.

use crate::ts_ms;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One stop-hook block row: did a memory row follow it? The session joins a
/// block to the round after it; row times come from the repo logs in
/// `aggregate`, never from here.
pub struct StopBlock {
    pub repo: String,
    pub event: String,
    pub ms: i64,
    pub session: String,
}

/// Everything `aggregate` needs, straight from the usage text.
pub struct Parsed {
    pub n: usize,
    pub bytes: usize,
    pub toks: usize,
    pub skipped: usize,
    pub by_event: HashMap<String, (usize, usize)>,
    pub by_client: HashMap<String, (usize, usize)>,
    pub by_id: HashMap<String, usize>,
    /// Repos each row id was pushed from — row statuses resolve through
    /// those repos' logs.
    pub id_repos: HashMap<String, Vec<String>>,
    pub blocks: Vec<StopBlock>,
    /// Kept rows feed the ask metrics in `aggregate`.
    pub kept: Vec<serde_json::Value>,
    /// First usage ts per repo — the "rows added since" denominator.
    pub first_seen: HashMap<String, i64>,
}

impl Parsed {
    /// Every repo the caller must load a log for before `aggregate`.
    pub fn repos(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.first_seen.keys().map(String::as_str).collect();
        let mut seen: HashSet<&str> = out.iter().copied().collect();
        for r in self
            .blocks
            .iter()
            .map(|b| b.repo.as_str())
            .chain(self.id_repos.values().flatten().map(String::as_str))
        {
            if seen.insert(r) {
                out.push(r);
            }
        }
        out
    }
}

/// Parse one `usage.jsonl` file's text. Torn lines are skipped, never an error.
pub fn parse(text: &str, state_path: &Path, tmp_dirs: &[PathBuf]) -> Parsed {
    let mut p = Parsed {
        n: 0,
        bytes: 0,
        toks: 0,
        skipped: 0,
        by_event: HashMap::new(),
        by_client: HashMap::new(),
        by_id: HashMap::new(),
        id_repos: HashMap::new(),
        blocks: vec![],
        kept: vec![],
        first_seen: HashMap::new(),
    };
    let in_tmp = |q: &Path| {
        tmp_dirs
            .iter()
            .any(|t| !t.as_os_str().is_empty() && q.starts_with(t))
    };
    let keep_tmp = in_tmp(state_path);
    for line in text.lines() {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !keep_tmp && v["repo"].as_str().is_some_and(|r| in_tmp(Path::new(r))) {
            p.skipped += 1;
            continue;
        }
        p.n += 1;
        p.bytes += v["bytes"].as_u64().unwrap_or(0) as usize;
        let t = v["est_tokens"].as_u64().unwrap_or(0) as usize;
        p.toks += t;
        let ev = v["event"].as_str().unwrap_or("?").to_string();
        let cl = v["client"].as_str().unwrap_or("?").to_string();
        if let (Some(repo), Some(ms)) = (v["repo"].as_str(), v["ts"].as_str().and_then(ts_ms)) {
            p.first_seen
                .entry(repo.to_string())
                .and_modify(|f| *f = (*f).min(ms))
                .or_insert(ms);
            if ev.starts_with("stop-") {
                p.blocks.push(StopBlock {
                    repo: repo.to_string(),
                    event: ev.clone(),
                    ms,
                    session: v["session"].as_str().unwrap_or("").to_string(),
                });
            }
        }
        count(&mut p.by_event, ev, t);
        count(&mut p.by_client, cl, t);
        for id in v["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i.as_str())
        {
            *p.by_id.entry(id.into()).or_insert(0) += 1;
            if let Some(repo) = v["repo"].as_str()
                && !p
                    .id_repos
                    .get(id)
                    .is_some_and(|rs| rs.iter().any(|r| r == repo))
            {
                p.id_repos.entry(id.into()).or_default().push(repo.into());
            }
        }
        p.kept.push(v);
    }
    p
}

fn count(into: &mut HashMap<String, (usize, usize)>, key: String, toks: usize) {
    into.entry(key)
        .and_modify(|e| {
            e.0 += 1;
            e.1 += toks;
        })
        .or_insert((1, toks));
}

#[cfg(test)]
mod tests {
    use super::parse;
    use std::path::{Path, PathBuf};

    fn tmp() -> Vec<PathBuf> {
        vec![PathBuf::from("/tmp")]
    }

    #[test]
    fn torn_lines_are_skipped_and_counts_hold() {
        let text = concat!(
            "{\"ts\":\"2026-09-26T00:00:00.000Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":3,\"ids\":[\"A\"]}\n",
            "not json\n",
            "{\"ts\":\"2026-09-26T00:01:00.000Z\",\"repo\":\"/work/real\",\"client\":\"codex\",\"event\":\"edit\",\"bytes\":20,\"est_tokens\":5,\"ids\":[\"A\",\"B\"]}\n",
        );
        let p = parse(text, Path::new("/work/state/usage.jsonl"), &tmp());
        assert_eq!((p.n, p.bytes, p.toks, p.skipped), (2, 30, 8, 0));
        assert_eq!(p.by_id.get("A"), Some(&2));
        assert_eq!(p.by_event.get("read"), Some(&(1, 3)));
        assert_eq!(p.first_seen.get("/work/real"), Some(&1790380800000));
        assert_eq!(p.repos(), vec!["/work/real"]);
    }

    #[test]
    fn temp_repos_skip_unless_state_is_scratch() {
        let line = |repo: &str| {
            format!(
                "{{\"ts\":\"2026-09-26T00:00:00.000Z\",\"repo\":\"{repo}\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":3,\"ids\":[]}}\n"
            )
        };
        let text = line("/tmp/bench.x") + &line("/work/real");
        let real = parse(text.as_str(), Path::new("/work/state/usage.jsonl"), &tmp());
        assert_eq!((real.n, real.skipped), (1, 1));
        let scratch = parse(text.as_str(), Path::new("/tmp/scratch/usage.jsonl"), &tmp());
        assert_eq!((scratch.n, scratch.skipped), (2, 0));
    }
}
