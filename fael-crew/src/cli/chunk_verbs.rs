//! `fael chunk approve|unapprove|accept|show` over several uids (plan fael-friction-2
//! chunk-verbs): every uid runs in order, one result line per uid, a failed one never
//! stops the rest. Flags (`--model`, `--client`) apply to every uid, and `--plan`
//! resolves each key on its own, so one bad label rejects alone too. One uid keeps the
//! old behaviour byte for byte — its error returns for `cmd` to word — while several
//! print each failure as its own line and the call ends with a `partial` summary when
//! any failed, so the rest stay saved without resending them.

use crate::{Args, Repo};
use fael_crew::{Owner, Store};
use std::process::ExitCode;

/// What a partially failed call ends with on stderr: the per-uid lines on stdout say
/// which uid failed, this says how many. Carries the `chunks rejected` needle, so usage
/// counts the call apart from a clean one (`partial`, like batch close).
fn summary(failed: usize, n: usize) -> String {
    format!("rejected: {failed} of {n} chunks rejected — the rest succeeded")
}

/// One uid's failure, worded as a lone call would be (a foreign uid names its repo).
fn failed(r: &Repo, e: String) -> String {
    crate::chunk::row_id(r, e)
}

/// The uid `key` names: with `--plan` plan-scoped (`uid_in` answers a label two
/// plans share and checks a uid it is given; a `plan/label` key resolves
/// through its own plan), else the key as `Store` resolves it. Called once per
/// key by every loop below, so one bad key never stops the rest — its own line
/// words the failure. `chunk after`/`unafter` reuse it for both keys in
/// `chunk::in_plan`.
pub(crate) fn resolve(s: &Store, plan: Option<&str>, key: &str) -> Result<String, String> {
    match plan {
        Some(p) => s.uid_in(key, p),
        None => Ok(key.to_string()),
    }
}

/// `fael chunk approve <uid>… [--model M] [--client C]`: the flags apply to all.
pub(crate) fn approve(a: &Args, r: &Repo, uids: &[&str], now: &str) -> Result<(), String> {
    if uids.is_empty() {
        return Err(bad(Some("approve")));
    }
    a.only("chunk approve", &["model", "client", "plan"])?;
    let plan = a.one("plan");
    let mut s = Store::open(&crate::plan::db(r))?;
    let (m, c) = (a.one("model"), a.one("client"));
    if uids.len() == 1 {
        let uid = resolve(&s, plan.as_deref(), uids[0])?;
        let text = s.approve(&uid, Some((m.as_deref(), c.as_deref())), now)?;
        println!("chunk {uid} → {text}");
        return Ok(());
    }
    let mut bad = 0;
    for key in uids {
        match resolve(&s, plan.as_deref(), key) {
            Err(e) => {
                bad += 1;
                println!("{}", failed(r, e));
            }
            Ok(uid) => match s.approve(&uid, Some((m.as_deref(), c.as_deref())), now) {
                Ok(text) => println!("chunk {uid} → {text}"),
                Err(e) => {
                    bad += 1;
                    println!("{}", failed(r, e));
                }
            },
        }
    }
    if bad > 0 {
        return Err(summary(bad, uids.len()));
    }
    Ok(())
}

/// `fael chunk unapprove <uid>…`: every uid leaves the queue.
pub(crate) fn unapprove(a: &Args, r: &Repo, uids: &[&str], now: &str) -> Result<(), String> {
    if uids.is_empty() {
        return Err(bad(Some("unapprove")));
    }
    a.only("chunk unapprove", &["plan"])?;
    let plan = a.one("plan");
    let mut s = Store::open(&crate::plan::db(r))?;
    if uids.len() == 1 {
        let uid = resolve(&s, plan.as_deref(), uids[0])?;
        let text = s.approve(&uid, None, now)?;
        println!("chunk {uid} → {text}");
        return Ok(());
    }
    let mut bad = 0;
    for key in uids {
        match resolve(&s, plan.as_deref(), key) {
            Err(e) => {
                bad += 1;
                println!("{}", failed(r, e));
            }
            Ok(uid) => match s.approve(&uid, None, now) {
                Ok(text) => println!("chunk {uid} → {text}"),
                Err(e) => {
                    bad += 1;
                    println!("{}", failed(r, e));
                }
            },
        }
    }
    if bad > 0 {
        return Err(summary(bad, uids.len()));
    }
    Ok(())
}

/// `fael chunk accept <uid>…`: every uid in review lands as done. `--merged` (hidden, no
/// uids) takes every review chunk whose run branch git says is on the base: the merge is
/// the owner's own act, so this only records it.
pub(crate) fn accept(a: &Args, r: &Repo, uids: &[&str], now: &str) -> Result<(), String> {
    if a.has("merged") {
        a.only("chunk accept", &["merged"])?;
        if !uids.is_empty() {
            return Err(bad(Some("accept")));
        }
        let found = crate::board::branches::merged_reviews(r)?;
        if found.is_empty() {
            println!("no review chunk on a merged branch");
            return Ok(());
        }
        let keys: Vec<&str> = found.iter().map(String::as_str).collect();
        return accept(&a.without("merged"), r, &keys, now);
    }
    if uids.is_empty() {
        return Err(bad(Some("accept")));
    }
    a.only("chunk accept", &["plan"])?;
    let plan = a.one("plan");
    let mut s = Store::open(&crate::plan::db(r))?;
    if uids.len() == 1 {
        let uid = resolve(&s, plan.as_deref(), uids[0])?;
        s.owner(&uid, Owner::Accept, None, now)?;
        println!("chunk {uid} → done");
        return Ok(());
    }
    let mut bad = 0;
    for key in uids {
        match resolve(&s, plan.as_deref(), key) {
            Err(e) => {
                bad += 1;
                println!("{}", failed(r, e));
            }
            Ok(uid) => match s.owner(&uid, Owner::Accept, None, now) {
                Ok(()) => println!("chunk {uid} → done"),
                Err(e) => {
                    bad += 1;
                    println!("{}", failed(r, e));
                }
            },
        }
    }
    if bad > 0 {
        return Err(summary(bad, uids.len()));
    }
    Ok(())
}

/// `fael chunk show <uid>… [--json]`: every chunk prints in order. A show is several
/// lines, so failures print as their reject line where that chunk's show would be.
pub(crate) fn show(a: &Args, r: &Repo, keys: &[&str]) -> Result<ExitCode, String> {
    if keys.is_empty() {
        return Err(bad(Some("show")));
    }
    a.only("chunk show", &["json", "plan"])?;
    let plan = a.one("plan");
    // chunk_show reads no --plan: each key has become its uid by then
    let a = &a.without("plan");
    // every key resolves on its own: a bad one is its own line, the rest show
    let resolved: Vec<Result<String, String>> = match plan.as_deref() {
        Some(p) => {
            let s = Store::open(&crate::plan::db(r))?;
            keys.iter().map(|k| resolve(&s, Some(p), k)).collect()
        }
        None => keys.iter().map(|k| Ok(k.to_string())).collect(),
    };
    if let [uid] = resolved.as_slice() {
        return crate::chunk_show::show(a, r, &uid.clone()?);
    }
    let mut bad = 0;
    for item in resolved {
        if let Err(e) = item.and_then(|uid| crate::chunk_show::show(a, r, &uid).map(|_| ())) {
            bad += 1;
            println!("{}", failed(r, e));
        }
    }
    if bad > 0 {
        return Err(summary(bad, keys.len()));
    }
    Ok(ExitCode::SUCCESS)
}

/// Wrong arguments: the verb's own synopsis when it is a known verb; an
/// unknown verb names itself, the closest real verb, and the short guide —
/// never the whole verb list. Line one keeps the `unknown chunk verb` needle
/// the usage log classifies by (the MCP side words it the same way).
pub(crate) fn bad(verb: Option<&str>) -> String {
    if verb.is_some_and(|v| crate::chunk::VERBS.contains(&v)) {
        return fael_core::help::wrong_args("chunk", verb, crate::chunk::USAGE);
    }
    let v = verb.unwrap_or_default();
    if v.is_empty() {
        return format!(
            "rejected: chunk needs a verb\n{}",
            fael_core::help::bare("chunk").unwrap_or_default()
        );
    }
    let hint = crate::chunk::VERBS
        .iter()
        .map(|n| (fael_core::levenshtein(n, v), n))
        .filter(|(d, _)| (1..=2).contains(d))
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| format!(" — did you mean {n}?"))
        .unwrap_or_default();
    // the `try` tail keeps the `chunk list|ls` retype answering (it matches on
    // it); line one keeps the `unknown chunk verb` needle the usage log
    // classifies by (the MCP side words it the same way).
    format!(
        "rejected: unknown chunk verb {v:?}{hint} — try 'fael chunk --help'\n{}",
        fael_core::help::bare("chunk").unwrap_or_default()
    )
}
