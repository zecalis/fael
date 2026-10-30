//! A lifecycle decision and its append share the same store lock. A caller's
//! snapshot can predate another writer, so load the tree/journal union only
//! after locking and keep the lock until the resulting event is appended.

use super::{Log, atomic, lock};
use crate::Row;
use std::path::{Path, PathBuf};

pub(super) struct Transaction<'a> {
    pub log: Log,
    fael: &'a Path,
    journal: Option<&'a Path>,
    _guard: std::fs::File,
}

impl<'a> Transaction<'a> {
    pub fn open(fael: &'a Path, journal: Option<&'a Path>) -> Result<Self, String> {
        let guard = lock(journal.unwrap_or(fael))?;
        let log = crate::fold_bumps(crate::journal::union_paths(fael, journal).0);
        Ok(Self {
            log,
            fael,
            journal,
            _guard: guard,
        })
    }

    pub fn append(&self, rows: &[(&Row, bool)]) -> Result<Vec<PathBuf>, String> {
        let paths = atomic::append_locked(self.journal.unwrap_or(self.fael), rows)?;
        crate::work::elsewhere::register_store(self.fael, self.journal);
        Ok(paths)
    }
}
