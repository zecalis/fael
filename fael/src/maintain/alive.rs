//! Files alive on a row's own branch. A row filed on a branch that has not
//! merged names files the current checkout does not have yet — the work is
//! alive there, not gone, so `[Gone]`/`[PartGone]` must not flag it. Every
//! `(branch, path)` goes through one `git cat-file --batch-check` started on
//! the first ask, so a doctor run spawns one git however many branches its
//! rows name; each (branch, path) is asked once.
//! ponytail: a merged branch still sitting in the clone keeps its files
//! "alive" after main deletes them — `[Merged]` already says to delete it.

use crate::core;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub(crate) struct BranchFiles<'a> {
    root: &'a Path,
    head: Option<String>,
    /// `(branch, path)` → alive on that branch's tip; each path is asked once.
    alive: RefCell<HashMap<(String, String), bool>>,
    /// The batch-check process, started once on the first ask; `None` after
    /// it failed — then every file reads as not alive, no respawn.
    probe: RefCell<Option<Option<Probe>>>,
}

impl<'a> BranchFiles<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        BranchFiles {
            root,
            head: super::head_branch(root),
            alive: RefCell::new(HashMap::new()),
            probe: RefCell::new(None),
        }
    }

    /// The row's missing files minus those on its branch's tip (local branch
    /// or origin's). A row on HEAD's branch, or with no branch, keeps every
    /// missing file — the checkout is its branch.
    pub(crate) fn missing<'r>(&self, row: &'r core::Row, gone: Vec<&'r str>) -> Vec<&'r str> {
        let Some(b) = row.branch().filter(|b| Some(*b) != self.head.as_deref()) else {
            return gone;
        };
        let mut alive = self.alive.borrow_mut();
        let key = |f: &str| (b.to_string(), f.to_string());
        for f in &gone {
            alive
                .entry(key(f))
                .or_insert_with(|| self.on_tip(b, f) || self.on_tip(&format!("origin/{b}"), f));
        }
        gone.into_iter().filter(|f| !alive[&key(f)]).collect()
    }

    /// Is `file` a file (or submodule) on `rev`'s tip? A failed probe says no.
    fn on_tip(&self, rev: &str, file: &str) -> bool {
        if file.contains('\n') {
            return false;
        }
        let mut slot = self.probe.borrow_mut();
        let p = slot.get_or_insert_with(|| Probe::start(self.root));
        let answer = p.as_mut().and_then(|p| p.ask(&format!("{rev}:{file}")));
        if answer.is_none() {
            *p = None;
        }
        matches!(answer.as_deref(), Some("blob" | "commit"))
    }
}

/// `git cat-file --batch-check=%(objecttype)`: one line in, one line out —
/// the object's type, or `<name> missing`. Flushes per object by default.
struct Probe {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Probe {
    fn start(root: &Path) -> Option<Probe> {
        let mut child = Command::new("git")
            .args(["cat-file", "--batch-check=%(objecttype)"])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take()?);
        Some(Probe {
            child,
            input,
            output,
        })
    }

    fn ask(&mut self, name: &str) -> Option<String> {
        let input = self.input.as_mut()?;
        writeln!(input, "{name}")
            .and_then(|()| input.flush())
            .ok()?;
        let mut line = String::new();
        (self.output.read_line(&mut line).ok()? > 0).then(|| line.trim_end().to_string())
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        drop(self.input.take()); // EOF ends cat-file
        let _ = self.child.wait();
    }
}
