//! Shared board read helpers: `git_output` and the parsed-ledger cache hook.
//! (`fael board --watch` retired step-2: the board is a single read.)

use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

type Parsed = (crate::core::Log, HashSet<String>);
type GitOut = Option<(Option<i32>, String)>;

/// `git <args>` in `dir`: its exit code and stdout.
pub(crate) fn git_output(dir: &Path, args: &[&str]) -> GitOut {
    let o = fael_core::git_cmd()
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    Some((
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    ))
}

/// A project's parsed ledger and its drifted ids.
pub(super) fn ledger(_root: &Path, parse: impl FnOnce() -> Parsed) -> Rc<Parsed> {
    Rc::new(parse())
}
