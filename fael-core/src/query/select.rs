use super::Filter;
use super::matching::{all_words, file_match, glob, is_md, lenient};
use crate::{Aliases, Log, Row, anchor, is_alias_row, is_carrier_row, resolve, to_matches};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;

/// Ids of closed rows (a close row, or a compact row's `closed` field).
pub fn closed(log: &Log) -> HashSet<&str> {
    let mut h: HashSet<&str> = log
        .closes
        .iter()
        .filter_map(|c| c.reference.as_deref())
        .collect();
    h.extend(
        log.rows
            .iter()
            .filter(|r| r.extra.contains_key("closed"))
            .map(|r| r.id.as_str()),
    );
    h
}

/// Supersede edges a restore row reverted, by superseder id — one row
/// supersedes at most one row, so the superseder names the edge (`restores`).
pub fn reverted(log: &Log) -> HashSet<&str> {
    log.rows
        .iter()
        .filter_map(|r| r.restores.as_deref())
        .collect()
}

/// Ids some newer row names in `supersedes`, minus the reverted edges: a
/// restored row is open again unless another still-active edge names it.
/// The one reader for hiding (find, push, doctor, hook) — an old reader that
/// subtracts nothing keeps hiding the restored row (over-hide, intended).
pub fn superseded(log: &Log) -> HashSet<&str> {
    super::successors(log).into_keys().collect()
}

/// What `add --urgent` asks for: `Unset` = not urgent, `End` = back of the
/// queue, `Before(id)` = just above that row — the midpoint with the row
/// above it, or half the top value when it is first (the queue starts at 1,
/// so halving never crosses zero).
#[derive(Debug, Default, Clone)]
pub enum Urgent {
    #[default]
    Unset,
    End,
    Before(String),
}

/// What `bump` does to the urgent queue: `Keep` leaves it, `Remove` clears it.
#[derive(Debug, Clone)]
pub enum UrgentChange {
    Keep,
    End,
    Before(String),
    Remove,
}

/// Open issues carrying an urgent number, most urgent first (number asc, id
/// desc breaks ties). Closed and superseded rows left the queue when they
/// left every list.
fn urgent_queue(log: &Log) -> Vec<(f64, &Row)> {
    let hide: HashSet<&str> = closed(log).union(&superseded(log)).copied().collect();
    let mut q: Vec<(f64, &Row)> = log
        .rows
        .iter()
        .filter(|r| !hide.contains(r.id.as_str()) && r.kind == "issue")
        .filter_map(|r| r.urgent_value().map(|u| (u, r)))
        .collect();
    q.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| b.1.id.cmp(&a.1.id)));
    q
}

/// The number an `--urgent` / `--urgent-before` ask resolves to. A move never
/// renumbers the other rows — only the moved row is rewritten (through
/// `bump`), so the append-only log stays append-only. The f64 ceiling
/// (~50 midpoints in one gap) needs no rebalance write: ranking still breaks
/// an exact tie by id desc, deterministically.
pub fn resolve_urgent(log: &Log, opt: &Urgent) -> Result<Option<f64>, String> {
    let q = urgent_queue(log);
    match opt {
        Urgent::Unset => Ok(None),
        Urgent::End => Ok(Some(q.last().map_or(1.0, |(u, _)| u + 1.0))),
        Urgent::Before(id) => {
            let t = resolve(log, id)?;
            if t.kind != "issue" {
                return Err(format!(
                    "rejected: the urgent queue holds issues — {id:?} is {}",
                    t.kind
                ));
            }
            let tu = t.urgent_value().ok_or_else(|| {
                format!("rejected: --urgent-before needs an urgent row — {id:?} has no number")
            })?;
            let pos = q.iter().position(|(_, r)| r.id == t.id).ok_or_else(|| {
                format!(
                    "rejected: --urgent-before needs an open row — {id:?} is closed or superseded"
                )
            })?;
            // the nearest number strictly above — a tie with the row above has
            // no gap, so skip past it rather than falling to `tu - 1.0`
            let above = q[..pos]
                .iter()
                .rev()
                .map(|(u, _)| *u)
                .find(|u| *u < tu)
                .unwrap_or(0.0);
            // the queue starts at 1, so halving the top never crosses zero; a
            // hand-written non-positive top falls back to one step above it
            Ok(Some(if above < tu {
                (above + tu) / 2.0
            } else {
                tu - 1.0
            }))
        }
    }
}

/// Freshness from the row alone — callers with a worktree (kickoff) pass the
/// mtime-aware closure instead.
pub fn fresh_ts(r: &Row) -> i64 {
    crate::ts_ms(&r.ts).unwrap_or(0)
}

fn kind_rank(r: &Row) -> u8 {
    match r.kind.as_str() {
        "issue" => 0,
        "decision" => 1,
        "note" => 2,
        _ => 3,
    }
}

/// One ordering for every list (chunk 3): to=reader, urgent, match tier,
/// kind, freshness, id. Importance only comes from explicit signals someone
/// set (`to`, `urgent`) or structure (file match, kind), never from text.
/// `tier` is the push match (exact file > same dir > shared key); every other
/// list passes 0. Deterministic: the same log and reader give the same order
/// on any machine.
pub fn cmp_rows(
    a: &Row,
    b: &Row,
    reader: Option<&str>,
    tier_a: usize,
    tier_b: usize,
    fresh_a: i64,
    fresh_b: i64,
) -> Ordering {
    let mine = |r: &Row| reader.is_some_and(|w| r.to_who().is_some_and(|t| to_matches(t, w)));
    match (mine(a), mine(b)) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    match (a.urgent_value(), b.urgent_value()) {
        (Some(x), Some(y)) => {
            let ord = x.total_cmp(&y);
            if ord != Ordering::Equal {
                return ord;
            }
        }
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        (None, None) => {}
    }
    match tier_a.cmp(&tier_b) {
        Ordering::Equal => {}
        ord => return ord,
    }
    match kind_rank(a).cmp(&kind_rank(b)) {
        Ordering::Equal => {}
        ord => return ord,
    }
    match fresh_b.cmp(&fresh_a) {
        Ordering::Equal => {}
        ord => return ord,
    }
    b.id.cmp(&a.id)
}

/// Sort rows under `cmp_rows`, computing each row's tier and freshness once.
/// Push passes its match tier; kickoff its mtime freshness; everyone else the
/// zero tier and `fresh_ts`.
pub fn ranked<'a>(
    rows: Vec<&'a Row>,
    reader: Option<&str>,
    tier: impl Fn(&Row) -> usize,
    fresh: impl Fn(&Row) -> i64,
) -> Vec<&'a Row> {
    let mut keyed: Vec<(&Row, usize, i64)> =
        rows.into_iter().map(|r| (r, tier(r), fresh(r))).collect();
    keyed.sort_by(|a, b| cmp_rows(a.0, b.0, reader, a.1, b.1, a.2, b.2));
    keyed.into_iter().map(|(r, _, _)| r).collect()
}

/// Postgres-style paging over a ranked list: skip `offset`, take `limit`.
/// Returns the page plus the pre-page total, so the cut line can count down
/// from it. Applied by `query()` (find/brief) and the kickoff CLI — never by
/// `find()`/`kickoff()` themselves, so kickoff keeps ranking the full set and
/// push/session-start (whose filters carry no paging) are untouched.
pub fn page(rows: Vec<&Row>, limit: Option<usize>, offset: usize) -> (Vec<&Row>, usize) {
    let total = rows.len();
    let page: Vec<&Row> = rows
        .into_iter()
        .skip(offset)
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    (page, total)
}

/// Rows matching `f`, ranked most actionable first. Closed and superseded
/// rows are hidden unless `f.all`.
pub fn find<'a>(log: &'a Log, f: &Filter) -> Vec<&'a Row> {
    let hide: HashSet<&str> = if f.all {
        HashSet::new()
    } else {
        closed(log).union(&superseded(log)).copied().collect()
    };
    let text = f.text.as_ref().map(|t| t.to_lowercase());
    let revisit = f.revisit.as_ref().map(|q| q.to_lowercase());
    let files: Vec<String> = f
        .files
        .iter()
        .map(|q| lenient(q).trim_end_matches('/').to_string())
        .collect();
    let out: Vec<&Row> = log
        .rows
        .iter()
        .filter(|r| {
            !hide.contains(r.id.as_str())
                && !is_alias_row(r)
                && !is_carrier_row(r)
                && f.kind.as_ref().is_none_or(|k| &r.kind == k)
                && f.by.as_ref().is_none_or(|b| &r.by == b)
                && f.to
                    .as_ref()
                    // either side may be a full writer id or its name part
                    .is_none_or(|t| {
                        r.to_who()
                            .is_some_and(|w| to_matches(w, t) || to_matches(t, w))
                    })
                && f.since.as_ref().is_none_or(|s| r.ts.as_str() >= s.as_str())
                && f.key
                    .as_ref()
                    .is_none_or(|g| r.key.as_deref().is_some_and(|k| glob(g, k)))
                && text.as_ref().is_none_or(|t| all_words(t, r))
                // `--revisit`: any revisit, or a substring of it
                && revisit.as_ref().is_none_or(|q| {
                    r.revisit()
                        .is_some_and(|v| q.is_empty() || v.to_lowercase().contains(q))
                })
                && (files.is_empty()
                    || r.files.iter().any(|rf| {
                        let rf = lenient(rf);
                        files.iter().any(|q| file_match(q, &rf))
                    }))
        })
        .collect();
    // no reader and no file match here — urgent, kind, freshness, id decide
    ranked(out, None, |_| 0, fresh_ts)
}

/// The files the row names that are paths no longer existing under `root`,
/// even through `al` (a rename only *adds* a present path, so a wrong pair
/// keeps one row too many and never hides one). Anchors never go.
pub fn gone_files<'a>(root: &Path, r: &'a Row, al: &Aliases) -> Vec<&'a str> {
    let gone =
        |f: &&String| anchor(f).is_none() && al.forward(f).iter().all(|p| !root.join(p).exists());
    r.files.iter().filter(gone).map(String::as_str).collect()
}

/// Every file the row names is gone (`gone_files`). A row with no files is
/// never gone. Such rows never push, so kickoff drops them too.
pub fn gone(root: &Path, r: &Row, al: &Aliases) -> bool {
    !r.files.is_empty() && gone_files(root, r, al).len() == r.files.len()
}

/// The session brief (kickoff, and `find` with no filter): the unfiltered
/// find, which already ranks urgent first, then issues, decisions, notes,
/// repo kinds.
pub fn brief<'a>(log: &'a Log, f: &Filter) -> Vec<&'a Row> {
    find(log, f)
}

/// A `<PREFIX><name>.md` path also names the `<prefix>:<name>` anchor, so
/// rows filed under the anchor (not a guessed code file) surface on a
/// kickoff of the doc (`PLAN-<name>.md` → `plan:<name>` is the
/// long-standing default, PLAN-fael-direction chunk 6). The anchor ref is
/// lowercased — everything identity-like is. Which prefixes count is config
/// (`[anchor] prefixes`, default `PLAN-`): fael itself knows no workflow.
/// A matched prefix that is not markdown decides too — a non-doc never
/// widens, it does not fall through to the next prefix.
fn plan_anchor(file: &str, prefixes: &[String]) -> Option<String> {
    let base = file.rsplit('/').next().unwrap_or(file);
    for pre in prefixes {
        let Some(stem) = base.strip_prefix(pre.as_str()) else {
            continue;
        };
        if !is_md(stem) {
            return None;
        }
        // `.md` is ASCII, so `len - 3` is a char boundary here
        let name = &stem[..stem.len() - 3];
        if name.is_empty() {
            return None;
        }
        let scheme: String = pre
            .to_lowercase()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if scheme.is_empty() {
            return None;
        }
        return Some(format!("{scheme}:{}", name.to_lowercase()));
    }
    None
}

/// How fresh a row is: the newer of the row itself and the last change to
/// any of its files (following renames) — kickoff order. Shared with the
/// session-start hook, so due rows surface there in the same order.
pub fn freshness<'a>(root: &'a Path, al: &'a Aliases) -> impl Fn(&Row) -> i64 + 'a {
    move |r: &Row| {
        let row_ms = crate::ts_ms(&r.ts).unwrap_or(0);
        r.files
            .iter()
            // freshness follows the rename: the old path is gone, the new one
            // is what the worktree last touched
            .flat_map(|f| al.forward(f))
            .filter_map(|f| {
                std::fs::metadata(root.join(f))
                    .and_then(|m| m.modified())
                    .ok()
            })
            .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .fold(row_ms, i64::max)
    }
}

/// What a session opens with (`fael kickoff`, the session-start hook): the brief minus
/// rows whose files are gone, open issues first, then everything else by how fresh it is —
/// the newer of the row itself and the last change to any of its files. So an old decision
/// about a file nobody touches sinks, and one about the file changed yesterday rises.
/// A doc path whose name matches a configured prefix widens the filter
/// with its anchor (see `plan_anchor`).
// ponytail: file mtime is the "current work" signal — no git spawn on session start; a fresh
// clone or checkout resets mtimes, then the order falls back to roughly newest-row first.
pub fn kickoff<'a>(
    log: &'a Log,
    f: &Filter,
    root: &Path,
    al: &Aliases,
    prefixes: &[String],
) -> Vec<&'a Row> {
    // a reference, so both halves below share it without a move
    let fresh = freshness(root, al);
    let fresh = &fresh;
    let rows: Vec<&Row> = find(log, &widened(f, prefixes))
        .into_iter()
        .filter(|r| !gone(root, r, al))
        .collect();
    // due revisits wake up first, even from outside the file filter
    // (with_due) — the stable rank keeps find()'s order for ties
    let (due, rest) = super::with_due(log, rows, root, al);
    ranked(due, None, |_| 0, fresh)
        .into_iter()
        .chain(ranked(rest, None, |_| 0, fresh))
        .collect()
}

/// Widen a kickoff filter with `<prefix>:<name>` anchors (see `plan_anchor`).
fn widened(f: &Filter, prefixes: &[String]) -> Filter {
    let mut out = f.clone();
    for file in &f.files {
        if let Some(a) = plan_anchor(file, prefixes)
            && !out.files.iter().any(|q| q == &a)
        {
            out.files.push(a);
        }
    }
    out
}
