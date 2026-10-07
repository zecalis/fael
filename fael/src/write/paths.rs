//! `--files` read from a subdirectory: the cwd reading first, the repo-root
//! reading when only that one exists.

use crate::{core, hook};
use std::path::Path;

/// `fael add` from `sub/` with `--files sub/a.rs` (repo-root-relative) reads as `sub/sub/a.rs`;
/// when that is missing but the root reading exists, take it — a warning only when the arg does
/// not name `sub/` itself. The cwd reading wins whenever it exists (`a.rs` keeps `sub/a.rs`).
pub(super) fn root_relative(r: &crate::Repo, args: &[String], files: &mut [String]) -> Vec<String> {
    let mut warns = vec![];
    let sub = r.cwd.strip_prefix(&r.root).unwrap_or(&r.cwd);
    for (arg, f) in args.iter().zip(files.iter_mut()) {
        if hook::is_anchor(f) || core::is_glob(f) || r.root.join(&*f).exists() {
            continue;
        }
        if let Ok(v) = core::normalize_files(std::slice::from_ref(arg), &r.root, &r.root)
            && let Some(alt) = v.into_iter().next()
            && r.root.join(&alt).exists()
        {
            if !Path::new(arg).starts_with(sub) {
                warns.push(format!(
                    "warning: {arg:?} is not under {sub:?} — resolved from repo root as {alt:?}"
                ));
            }
            *f = alt;
        }
    }
    warns
}
