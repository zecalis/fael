//! Yield per line kind (PLAN-fael-say-gate chunk 3): of the lines a hook
//! said (`said` on its usage line), how many the same session acted on
//! after. Earned per kind (SPEC-fael-say-gate):
//!
//! - `row` / `brief` / `note` — the id was in context at an edit
//!   (`in_context`, `in_context_notes`) or retired within a day
//! - `ask` — the id was closed, superseded or bumped within a day
//! - `cited` (PLAN-fael-agent-ergonomics chunk 5) — the id was closed,
//!   superseded or bumped within a day, like `ask`: the line already names
//!   the close, so earning is the close itself
//! - `pointer` / `bodies` — a later pull (`found` line) by that key / an id
//!   (legacy: no hook says either now; old usage still tallies)
//! - `finding` (PLAN-fael-experience-loop chunk 2) — the session filed an
//!   issue naming the finding's file after it
//! - `fixed` / `fixcommit` (experience-loop chunk 5) — the session filed an issue after it
//!   (`fixcommit` is legacy: no hook says it now)
//! - `notice` — the session filed a row after it
//! - `overlap` / `checkpoint` / `briefdrift` — the session ran `fael chunk
//!   note` after it (a chunk note is no ledger row; its `call` usage line is
//!   the evidence)
//! - `retired` — the session filed a decision after it
//! - `held` — no hook says it now; old entries earn like `ask`
//!
//! The kinds are `SayKind` (`kind.rs`), shared with the hook that says them:
//! the rule per kind is `SayKind::earned`, and the tally below matches on it
//! with no `_` arm, so a kind cannot be said and silently go untallied.
//!
//! The push no longer says `count`, `merge`, `carry` or `promote` (#247,
//! #338): their old usage entries are skipped, not tallied.
//!
//! An upper bound: the agent may have done it anyway — use it only to cut.
//!
//! Cost and event (`said_yield.rs`): an entry may carry `t`, its share of the
//! line's `est_tokens` (the entries of one line sum to the line's tokens), and
//! the usage line names its `event`. Each kind keeps `tokens` — the sum of `t`,
//! 0 for entries written before `t` existed, so tokens are real only from that
//! change on — and `by_event`: said, earned and tokens per event. Earned by
//! event uses the kind's own rule, so yield by event is retroactive: it reads
//! every usage line in the window. Tokens ride on the entry the line names; a
//! `brief` entry names its rows by the line's `ids`, and its `t` is counted
//! once, on the first.
//!
//! Pure: kept usage rows and loaded logs in.

use super::acted::count as acted_count;
use super::capture::mine;
use super::kind::{Earned, SayKind};
use super::parse::Parsed;
use super::retire::{RETIRE_WINDOW_MS, retire_times};
use super::said_yield::tally;
pub use super::said_yield::{EventYield, KindYield};
use crate::{Log, Row, ts_ms};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A pull's query, from its `found` line.
pub(super) struct Pull<'a> {
    pub(super) ms: i64,
    pub(super) key: Option<&'a str>,
    pub(super) files: Vec<&'a str>,
    /// The id a `find <id>` named.
    pub(super) id: Option<&'a str>,
}

type Session<'a> = (&'a str, &'a str);

/// The session id a usage line carries: a hook's transcript path
/// (`…/<id>.jsonl`) and a pull's bare `$CLAUDE_CODE_SESSION_ID` share the stem.
fn session(v: &serde_json::Value) -> Option<&str> {
    let s = v["session"].as_str()?;
    Some(
        std::path::Path::new(s)
            .file_stem()
            .and_then(|f| f.to_str())
            .unwrap_or(s),
    )
}

pub(super) fn strs<'a>(v: &'a serde_json::Value, k: &str) -> impl Iterator<Item = &'a str> {
    v[k].as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i.as_str())
}

/// A pull that takes the call a count line printed. Its key is
/// `<files>|<what>`: `file` (`fael find --files <files>`), `dir:<dirs>`
/// (`--files src/`), `key:<key>` (`--key <key>`), `keys` (`--key <key>`, up to
/// three named — any key pull counts, an upper bound like the rest). The
/// pull's files are normalised (`src/` reads `src`), so a path covers a file
/// at a `/` boundary: `src` covers `src/a.rs`, `src/a` does not.
pub(super) fn counted(key: &str, p: &Pull) -> bool {
    let (files, what) = key.split_once('|').unwrap_or((key, "file"));
    let covers = |f: &str| {
        let f = f.trim_end_matches('/');
        files
            .split(',')
            .any(|k| k == f || k.strip_prefix(f).is_some_and(|r| r.starts_with('/')))
    };
    match what.strip_prefix("key:") {
        Some(k) => p.key == Some(k),
        None if what == "keys" => p.key.is_some(),
        None => p.files.iter().any(|f| covers(f)),
    }
}

/// `ask` split by whether its row sat on a hub file — the same earned, so the
/// two read against one bar. Only asks whose push recorded `feat` count.
pub const ASK_SPLIT: [&str; 2] = ["ask:hub", "ask:file"];

/// Whether the session filed a row `pick` takes after `ms`.
fn filed_after(
    log: Option<&Log>,
    s: Option<Session>,
    ms: i64,
    pick: impl Fn(&Row) -> bool,
) -> bool {
    log.is_some_and(|l| {
        l.rows.iter().any(|r| {
            // cheap tests first: the log is scanned once per said entry
            pick(r)
                && r.session().is_some()
                && s.is_some_and(|(_, s)| mine(r, s, None))
                && ts_ms(&r.ts).is_some_and(|t| t >= ms)
        })
    })
}

/// When a usage line is an accepted `fael chunk note` call.
fn chunk_note_ms(v: &serde_json::Value) -> Option<i64> {
    let is = v["event"] == "call" && v["cmd"] == "chunk" && v["sub"] == "note";
    (is && v["outcome"] == "ok")
        .then(|| v["ts"].as_str().and_then(ts_ms))
        .flatten()
}

/// What each session did, read off its usage lines: ids in context at an
/// edit, pulls, and the time of its last accepted `fael chunk note`.
#[allow(clippy::type_complexity)]
fn index(
    parsed: &Parsed,
) -> (
    HashSet<(Session<'_>, &str)>,
    HashMap<Session<'_>, Vec<Pull<'_>>>,
    HashMap<Session<'_>, i64>,
) {
    let mut in_ctx: HashSet<(Session, &str)> = HashSet::new();
    let mut pulls: HashMap<Session, Vec<Pull>> = HashMap::new();
    // the latest accepted chunk note per session
    let mut last_note: HashMap<Session, i64> = HashMap::new();
    for v in &parsed.kept {
        let (Some(repo), Some(s)) = (v["repo"].as_str(), session(v)) else {
            continue;
        };
        if let Some(ms) = chunk_note_ms(v) {
            last_note
                .entry((repo, s))
                .and_modify(|m| *m = (*m).max(ms))
                .or_insert(ms);
        }
        if v["event"] == "in-context" {
            let ids = strs(v, "in_context").chain(strs(v, "in_context_notes"));
            in_ctx.extend(ids.map(|id| ((repo, s), id)));
        } else if v.get("found").is_some()
            && let Some(ms) = v["ts"].as_str().and_then(ts_ms)
        {
            let q = &v["q"];
            pulls.entry((repo, s)).or_default().push(Pull {
                ms,
                key: q["key"].as_str(),
                files: strs(q, "files").collect(),
                id: q["id"].as_str(),
            });
        }
    }
    (in_ctx, pulls, last_note)
}

/// Each repo's note ids, so a `row` said entry joins by lookup, not by scan.
fn note_ids(logs: &HashMap<String, Log>) -> HashMap<&str, HashSet<&str>> {
    logs.iter()
        .map(|(repo, l)| {
            let notes = l.rows.iter().filter(|r| r.kind == "note");
            (repo.as_str(), notes.map(|r| r.id.as_str()).collect())
        })
        .collect()
}

pub(super) fn yields(parsed: &Parsed, logs: &HashMap<String, Log>) -> BTreeMap<String, KindYield> {
    let (in_ctx, pulls, last_note) = index(parsed);
    let gone: HashMap<&str, HashMap<&str, i64>> = logs
        .iter()
        .map(|(repo, log)| (repo.as_str(), retire_times(log)))
        .collect();
    let notes = note_ids(logs);
    let mut out: BTreeMap<String, KindYield> = SayKind::buckets()
        .map(|k| (k.to_string(), KindYield::default()))
        .collect();
    for v in &parsed.kept {
        let (Some(repo), Some(ms), Some(said)) = (
            v["repo"].as_str(),
            v["ts"].as_str().and_then(ts_ms),
            v["said"].as_array(),
        ) else {
            continue;
        };
        let s = session(v).map(|s| (repo, s));
        let log = logs.get(repo);
        let retired = |id: &str| {
            gone.get(repo)
                .and_then(|t| t.get(id))
                .is_some_and(|g| *g >= ms && *g - ms <= RETIRE_WINDOW_MS)
        };
        let noted = s.and_then(|s| last_note.get(&s)).is_some_and(|n| *n >= ms);
        let pulled = |hit: &dyn Fn(&Pull) -> bool| {
            s.and_then(|s| pulls.get(&s))
                .is_some_and(|ps| ps.iter().any(|p| p.ms >= ms && hit(p)))
        };
        // a brief names no key: its rows are the line's `ids`
        // the entry's `t` goes on the first of the entries it expands to
        let event = v["event"].as_str();
        let entries = said.iter().flat_map(|e| {
            let (kind, key) = (e["kind"].as_str().unwrap_or(""), e["key"].as_str());
            let t = e["t"].as_u64().unwrap_or(0) as usize;
            match (kind, key) {
                ("brief", None) => strs(v, "ids")
                    .enumerate()
                    .map(|(i, id)| ("brief", id, if i == 0 { t } else { 0 }))
                    .collect(),
                _ => vec![(kind, key.unwrap_or(""), t)],
            }
        });
        for (name, key, tokens) in entries {
            // a name no hook says any more (count, merge, carry, promote) is skipped
            let Some(kind) = SayKind::parse(name) else {
                continue;
            };
            // the generic clause names no row: nothing to join it to
            if kind == SayKind::Ask && key == "*" {
                continue;
            }
            let used = |id: &str| s.is_some_and(|s| in_ctx.contains(&(s, id))) || retired(id);
            let note = kind == SayKind::Row && notes.get(repo).is_some_and(|n| n.contains(key));
            let bucket = if note { "note" } else { kind.name() };
            let earned = match kind.earned() {
                Earned::InContext => used(key),
                Earned::Retired => retired(key),
                // an issue the session filed on the finding's file after it
                Earned::FiledIssueOnFile => filed_after(log, s, ms, |r| {
                    r.kind == "issue" && r.files.iter().any(|f| f == key)
                }),
                Earned::FiledIssue => filed_after(log, s, ms, |r| r.kind == "issue"),
                Earned::FiledDecision => filed_after(log, s, ms, |r| r.kind == "decision"),
                Earned::FiledAny => filed_after(log, s, ms, |_| true),
                Earned::ChunkNote => noted,
                Earned::Pulled => pulled(&|p| p.key == Some(key)),
                Earned::PulledId => pulled(&|p| p.id.is_some()),
            };
            tally(&mut out, bucket, event, earned, tokens);
            // the same ask split by the file it sat on: `hub` as the push
            // recorded it (`feat`), so the split never re-derives a hub
            if let Some(hub) = (kind == SayKind::Ask)
                .then(|| v["feat"][key]["hub"].as_bool())
                .flatten()
            {
                tally(
                    &mut out,
                    ASK_SPLIT[usize::from(!hub)],
                    event,
                    earned,
                    tokens,
                );
            }
        }
    }
    // r2: stamped retires ride on `held`, beside the unchanged `earned`
    // rule above — rows before r1 have no stamp and never count here.
    let acted = acted_count(logs, &parsed.first_seen);
    if !acted.is_empty()
        && let Some(held) = out.get_mut("held")
    {
        held.acted = acted;
    }
    out
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    pub(in crate::stats) fn rows(jsonl: &str) -> Vec<crate::Row> {
        let (mut out, mut w) = (vec![], vec![]);
        crate::log::parse(jsonl.as_bytes(), "t.jsonl", &mut out, &mut w);
        out
    }

    #[test]
    fn each_kind_earns_on_its_own_outcome() {
        let row = |id: &str, kind: &str, extra: &str| {
            format!(
                "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"a.rs\"]{extra}}}\n"
            )
        };
        // N is a note; F is filed by session s1 after the notice; A is closed
        // and G is closed too, the latter earning only the cited line
        let log = Log {
            rows: rows(
                &(row("D", "decision", "")
                    + &row("N", "note", "")
                    + &row("A", "issue", "")
                    + &row("G", "issue", "")
                    + &row("U", "decision", "")
                    + "{\"v\":1,\"id\":\"F\",\"ts\":\"2026-09-26T00:09:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[\"a.rs\"],\"session\":\"s1\"}\n"),
            ),
            closes: rows(
                "{\"v\":1,\"id\":\"C\",\"ts\":\"2026-09-26T00:08:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"A\"}\n{\"v\":1,\"id\":\"C2\",\"ts\":\"2026-09-26T00:09:30Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"G\"}\n",
            ),
            ..Log::default()
        };
        let line = |min: u8, rest: &str| {
            format!(
                "{{\"ts\":\"2026-09-26T00:0{min}:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",{rest}}}\n"
            )
        };
        // a pull knows only $CLAUDE_CODE_SESSION_ID, the hook's transcript stem
        let pull = |min: u8, rest: &str| {
            format!(
                "{{\"ts\":\"2026-09-26T00:0{min}:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"s1\",\"event\":\"find\",{rest}}}\n"
            )
        };
        // a brief names no key: its rows are the line's ids
        let usage = line(
            0,
            r#""event":"session-start","ids":["D","U"],"said":[{"kind":"brief"}]"#,
        ) + &line(
            1,
            r#""event":"edit","ids":["D","N","U"],"said":[{"kind":"row","key":"D"},{"kind":"row","key":"N"},{"kind":"row","key":"U"},{"kind":"bodies"},{"kind":"ask","key":"A"},{"kind":"ask","key":"*"},{"kind":"cited","key":"G"},{"kind":"finding","key":"a.rs"},{"kind":"finding","key":"z.rs"},{"kind":"notice"},{"kind":"fixed"},{"kind":"fixcommit"}]"#,
        ) + &line(
            2,
            r#""event":"prompt","ids":[],"said":[{"kind":"pointer","key":"k:x"},{"kind":"pointer","key":"k:y"}]"#,
        ) + &line(
            3,
            r#""event":"in-context","ids":[],"in_context":["D"],"in_context_notes":["N"]"#,
        ) + &pull(4, r#""found":["D"],"q":{"key":"k:x"}"#)
            + &pull(5, r#""found":["D"],"q":{"files":["b.rs"]}"#);
        let p = super::super::parse::parse(
            &usage,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let y = yields(&p, &HashMap::from([("/w/r".to_string(), log)]));
        let got = |k: &str| (y[k].said, y[k].earned);
        assert_eq!(got("row"), (2, 1), "D in context, U never");
        assert_eq!(got("note"), (1, 1));
        assert_eq!(got("ask"), (1, 1), "A closed, * not counted");
        assert_eq!(got("pointer"), (2, 1), "k:x pulled, k:y never");
        assert_eq!(got("bodies"), (1, 0), "no find by id");
        assert_eq!(got("cited"), (1, 1), "G closed after the commit line");
        assert_eq!(
            got("finding"),
            (2, 1),
            "F filed on a.rs by s1, none on z.rs"
        );
        assert_eq!(got("notice"), (1, 1), "F filed by s1 after it");
        assert_eq!([got("fixed"), got("fixcommit")], [(1, 1); 2], "F after");
        assert_eq!(got("brief"), (2, 1), "D in context, U never");
        // a pull's outcome line is no injection
        assert_eq!(p.n, 3, "session-start + edit + prompt");
    }
}
