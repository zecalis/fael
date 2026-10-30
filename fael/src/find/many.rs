//! `find a b c` and MCP `ids: [...]`: several bodies in one call, so reading
//! what a key list showed is one round, not one per row. Like batch `close`,
//! a bad id reports alone and the rest still print.

use crate::journal::BranchMap;
use crate::{Args, Repo, core, journal};

pub(crate) struct Pulled {
    pub text: String,
    pub errors: Vec<String>,
    pub shown: Vec<String>,
}

/// Every id's body (or its JSON line), in the order asked — all of them in
/// one call (c2: no budget cut; an explicit list of ids is the agent naming
/// exactly what it needs). An id that is missing or ambiguous is named in
/// `errors`; a non-id-shaped token keeps the `is not an id` wording (the
/// same internal `Missing`, presentation alias only). JSON is the machine
/// shape. `spell` stays for callers that name the next call on a cut, but no
/// cut is printed — every found body shows.
pub(crate) fn pull(
    r: &Repo,
    base: core::Log,
    tags: BranchMap,
    ids: &[String],
    json: bool,
    _spell: &dyn Fn(&[String]) -> String,
) -> Pulled {
    let log = base;
    let mut found: Vec<core::Row> = vec![];
    let mut errors = vec![];
    let mut chunks = String::new();
    for id in ids {
        if id.contains('/') {
            errors.push(format!(
                "rejected: {id:?} is a path, not an id — several --files go comma-joined: --files a,b"
            ));
            continue;
        }
        // Single canonical lookup: `resolve_identity` decides One/Many/Missing;
        // `looks_like_id` below only picks the miss wording, never the outcome.
        match core::resolve_identity(&log, id) {
            core::Ref::One(row) => found.push(row.clone()),
            core::Ref::Many(rows) => {
                let owned: Vec<core::Row> = rows.into_iter().cloned().collect();
                errors.push(super::reject_many(id, &owned));
            }
            core::Ref::Missing if core::looks_like_id(id) => {
                match (!json).then(|| super::owner::chunk_here(r, id)).flatten() {
                    Some(c) => chunks.push_str(&c),
                    None => errors.push(super::reject_missing(r, &log, id)),
                }
            }
            core::Ref::Missing => {
                errors.push(format!(
                    "rejected: {id:?} is not an id — copy it from fael find"
                ));
            }
        }
    }
    if json {
        let text = found.iter().map(|r| format!("{}\n", r.to_line())).collect();
        let shown = found.iter().map(|r| r.id.clone()).collect();
        return Pulled {
            text,
            errors,
            shown,
        };
    }
    let rows: Vec<&core::Row> = found.iter().collect();
    // All ids in one call: no budget cut (an explicit id list names exactly
    // what the agent needs — truncating it only costs the round it saved).
    let text = journal::tag(core::render_full(&log, &rows, usize::MAX), &tags) + &chunks;
    let n = text.lines().filter(|l| l.starts_with("- [")).count();
    let shown = found.iter().take(n).map(|r| r.id.clone()).collect();
    Pulled {
        text,
        errors,
        shown,
    }
}

/// `fael find <id> <id> …`
pub(crate) fn find_many(a: &Args, ids: &[String]) -> Result<(), String> {
    let r = crate::repo()?;
    let (mut base, jtags) = journal::read(&r);
    crate::hook::mark_dormant(&r, &mut base);
    let spell = |left: &[String]| format!("fael find {}", left.join(" "));
    let p = pull(&r, base, jtags, ids, a.has("json"), &spell);
    print!("{}", p.text);
    // like batch close: the bad ids ride stdout beside what saved
    p.errors.iter().for_each(|e| println!("{e}"));
    match p.errors.len() {
        0 => Ok(()),
        n => Err(format!(
            "rejected: {n} of {} ids not shown — the rest printed",
            ids.len()
        )),
    }
}
