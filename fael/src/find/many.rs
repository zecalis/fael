//! `find a b c` and MCP `ids: [...]`: several bodies in one call, so reading
//! what a key list showed is one round, not one per row. Like batch `close`,
//! a bad id reports alone and the rest still print.

use super::branches::{self, BranchMap};
use crate::refs::Wide;
use crate::{Args, Repo, core, journal, refs};

pub(crate) struct Pulled {
    pub text: String,
    pub errors: Vec<String>,
    pub shown: Vec<String>,
}

/// Every id's body (or its JSON line), in the order asked. An id that is
/// missing, ambiguous or not id-shaped is named in `errors`.
pub(crate) fn pull(
    r: &Repo,
    base: core::Log,
    jtags: BranchMap,
    ids: &[String],
    json: bool,
) -> Pulled {
    let (mut log, mut tags) = (base, jtags);
    let mut p = Pulled {
        text: String::new(),
        errors: vec![],
        shown: vec![],
    };
    for id in ids {
        if !core::looks_like_id(id) {
            p.errors.push(format!(
                "rejected: {id:?} is not an id — copy it from fael find"
            ));
            continue;
        }
        let (l, wide, bt) = refs::resolve_wide(r, log, id);
        log = l;
        tags = journal::overlay(tags, bt);
        match wide {
            Wide::One(row) => {
                p.shown.push(row.id.clone());
                match json {
                    true => p.text.push_str(&format!("{}\n", row.to_line())),
                    false => p.text.push_str(&branches::tag(
                        core::render_full(&log, &[row.as_ref()], 10_000),
                        &tags,
                    )),
                }
            }
            Wide::Many(rows) => p.errors.push(super::reject_many(id, &rows)),
            Wide::Missing => p.errors.push(super::reject_missing(&log, id)),
        }
    }
    p
}

/// `fael find <id> <id> …`
pub(crate) fn find_many(a: &Args, ids: &[String]) -> Result<(), String> {
    let r = crate::repo()?;
    let (base, jtags) = journal::read(&r);
    let p = pull(&r, base, jtags, ids, a.has("json"));
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
