//! Calls per command and flag (PLAN cli-tiers chunk measure): which flags are
//! really used, read from the `event:"call"` lines that carry a `shape`
//! (`flags` — names only, never a value). `who: "human"` marks a call made
//! outside an agent session; it is counted here and nowhere in `friction`.
//! Pure: the parsed lines in, one struct out. Counts show use, not value.

use super::parse::Parsed;
use serde::Serialize;
use std::collections::BTreeMap;

/// One command (`flag` absent: every call to it) or one of its flags.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FlagUse {
    /// `cmd`, or `cmd sub` for a `chunk`/`plan` verb.
    pub cmd: String,
    pub flag: Option<String>,
    pub calls: usize,
    pub agent: usize,
    pub human: usize,
    pub ok: usize,
    pub help: usize,
    pub reject: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FlagUsage {
    /// Day of the first counted call line (`YYYY-MM-DD`), empty when none —
    /// the window the counts cover; older lines carry no flags.
    pub since: String,
    /// Most calls first.
    pub by_flag: Vec<FlagUse>,
}

pub(super) fn usage(parsed: &Parsed) -> FlagUsage {
    let mut m: BTreeMap<(String, Option<String>), FlagUse> = BTreeMap::new();
    let mut since = String::new();
    for v in parsed.kept.iter().filter(|v| v["event"] == "call") {
        let (Some(flags), Some(cmd), Some(ts)) =
            (v["flags"].as_array(), v["cmd"].as_str(), v["ts"].as_str())
        else {
            continue;
        };
        let day = ts.get(..10).unwrap_or("");
        if since.is_empty() || day < since.as_str() {
            since = day.to_string();
        }
        let by = match v["sub"].as_str() {
            Some(s) => format!("{cmd} {s}"),
            None => cmd.to_string(),
        };
        let flags = flags.iter().filter_map(|f| f.as_str());
        for flag in std::iter::once(None).chain(flags.map(Some)) {
            let key = (by.clone(), flag.map(String::from));
            let e = m.entry(key).or_insert_with(|| FlagUse {
                cmd: by.clone(),
                flag: flag.map(String::from),
                ..FlagUse::default()
            });
            e.calls += 1;
            if v["who"] == "human" {
                e.human += 1;
            } else {
                e.agent += 1;
            }
            match v["outcome"].as_str() {
                Some("ok") => e.ok += 1,
                Some("help") => e.help += 1,
                Some("reject") => e.reject += 1,
                _ => {}
            }
        }
    }
    let mut by_flag: Vec<FlagUse> = m.into_values().collect();
    by_flag.sort_by_key(|u| std::cmp::Reverse(u.calls)); // stable: ties stay cmd, flag order
    FlagUsage { since, by_flag }
}

#[cfg(test)]
mod tests {
    use super::super::parse::parse;
    use std::path::{Path, PathBuf};

    #[test]
    fn counts_split_by_who_and_outcome_and_skip_rows_without_shape() {
        let text = [
            r#"{"ts":"2026-10-19T00:00:00.000Z","repo":"/w/r","client":"cli","event":"call","cmd":"find","outcome":"ok","flags":["recall"],"npos":0}"#,
            r#"{"ts":"2026-10-20T00:00:00.000Z","repo":"/w/r","client":"cli","event":"call","cmd":"find","outcome":"reject","who":"human","flags":["recall"],"npos":0}"#,
            r#"{"ts":"2026-10-18T00:00:00.000Z","repo":"/w/r","client":"cli","event":"call","cmd":"find","outcome":"ok"}"#,
        ]
        .join("\n");
        let p = parse(
            &text,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let u = super::usage(&p);
        assert_eq!(u.since, "2026-10-19");
        assert_eq!(u.by_flag.len(), 2);
        let f = u.by_flag.iter().find(|x| x.flag.is_some()).unwrap();
        assert_eq!((f.calls, f.agent, f.human, f.ok, f.reject), (2, 1, 1, 1, 1));
    }
}
