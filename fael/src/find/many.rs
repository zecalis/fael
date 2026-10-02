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

/// Every id's body (or its JSON line), in the order asked, under the same
/// token `budget` as `--full` — the first body always shows, and the cut line
/// names the ids left, spelled by `spell` for this surface. An id that is
/// missing, ambiguous or not id-shaped is named in `errors`. JSON is the
/// machine shape and is not cut.
pub(crate) fn pull(
    r: &Repo,
    base: core::Log,
    jtags: BranchMap,
    ids: &[String],
    json: bool,
    spell: &dyn Fn(&[String]) -> String,
) -> Pulled {
    let (mut log, mut tags) = (base, jtags);
    let mut found: Vec<core::Row> = vec![];
    let mut errors = vec![];
    for id in ids {
        if !core::looks_like_id(id) {
            errors.push(format!(
                "rejected: {id:?} is not an id — copy it from fael find"
            ));
            continue;
        }
        let (l, wide, bt) = refs::resolve_wide(r, log, id);
        log = l;
        tags = journal::overlay(tags, bt);
        match wide {
            Wide::One(row) => found.push(*row),
            Wide::Many(rows) => errors.push(super::reject_many(id, &rows)),
            Wide::Missing => errors.push(super::reject_missing(&log, id)),
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
    let left = |n: usize| spell(&found[n..].iter().map(|r| r.id.clone()).collect::<Vec<_>>());
    let cut = core::Cut {
        total: rows.len(),
        offset: 0,
        next: &left,
    };
    let text = branches::tag(
        core::render_full_page(&log, &rows, r.cfg.find_tokens, cut),
        &tags,
    );
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
    let (base, jtags) = journal::read(&r);
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
