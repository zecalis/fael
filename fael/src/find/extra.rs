//! A positional after `--files` that looks like a path or an anchor joins the
//! files list (`find --files a b` = `find --files a,b`). Anything else keeps
//! today's meaning — an id-shaped token stays an id, words stay text.

use super::{find, many};
use crate::{Args, core};
use std::process::ExitCode;

/// A path (`a/b`, the rule `many::pull` already uses) or a `scheme:ref` anchor
/// (`doc:pricing`, `plan:x` — no `/` needed) by `fael_core::anchor`. Never
/// copied here: the rule lives in fael-core.
pub(crate) fn is_fileish(s: &str) -> bool {
    s.contains('/') || fael_core::anchor(s).is_some()
}

/// `(fileish, rest)`: when `--files` was given, every fileish positional joins
/// the files list; the rest keep today's meaning. Without `--files`, nothing
/// moves — `find a/b` stays a text query, `find a b` stays an id pull.
pub(crate) fn split(rest: &[String], has_files: bool) -> (Vec<String>, Vec<String>) {
    if !has_files {
        return (vec![], rest.to_vec());
    }
    let mut extra = vec![];
    let mut keep = vec![];
    for s in rest {
        if is_fileish(s) {
            extra.push(s.clone());
        } else {
            keep.push(s.clone());
        }
    }
    (extra, keep)
}

/// `find` dispatch: fileish positionals after `--files` join the files list
/// instead of rejecting as paths; ids keep today's pull, words today's query.
pub(crate) fn dispatch(a: &Args, rest: &[String]) -> Result<ExitCode, String> {
    if rest.len() <= 1 {
        let (extra, keep) = split(rest, a.has("files"));
        if extra.is_empty() {
            return find(a, rest.first()).map(|()| ExitCode::SUCCESS);
        }
        debug_assert!(keep.is_empty());
        return find(&a.with_extra_files(&extra), None).map(|()| ExitCode::SUCCESS);
    }
    // `find lead worker worktree`: words, no id among them — one text query,
    // not three rejected ids (01M4XKM4); with `--files` an anchor among the
    // words is a file, not text.
    if rest
        .iter()
        .all(|w| !w.contains('/') && !core::looks_like_id(w))
    {
        let (extra, keep) = split(rest, a.has("files"));
        if extra.is_empty() {
            return find(a, Some(&rest.join(" "))).map(|()| ExitCode::SUCCESS);
        }
        let a2 = a.with_extra_files(&extra);
        return match keep.as_slice() {
            [] => find(&a2, None).map(|()| ExitCode::SUCCESS),
            _ => {
                let q = keep.join(" ");
                find(&a2, Some(&q)).map(|()| ExitCode::SUCCESS)
            }
        };
    }
    let (extra, keep) = split(rest, a.has("files"));
    if extra.is_empty() {
        many::find_many(a, rest).map(|()| ExitCode::SUCCESS)
    } else if keep.is_empty() {
        find(&a.with_extra_files(&extra), None).map(|()| ExitCode::SUCCESS)
    } else {
        many::find_many(a, &keep).map(|()| ExitCode::SUCCESS)
    }
}
