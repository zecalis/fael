//! Key suggestions (PLAN-fael-call-fit chunk k1): a keyless add on files
//! that already carry keys names the candidates (at most three, newest
//! first) and never picks one. Silent with a key or with no keyed file.

use crate::{core, hook};

const MAX: usize = 3;

/// Info line for the issue just filed (`log` is the log before it). None when
/// the row already has a key (set by the caller or by auto-key), or when no
/// keyed row shares a real file with it.
pub(crate) fn lines(log: &core::Log, row: &core::Row) -> Vec<String> {
    if row.key.is_some() {
        return vec![];
    }
    let mut hits: Vec<&core::Row> = log
        .rows
        .iter()
        .filter(|r| r.key.is_some())
        .filter(|r| {
            r.files
                .iter()
                .any(|f| row.files.contains(f) && !hook::is_anchor(f) && !core::is_glob(f))
        })
        .collect();
    hits.sort_by(|a, b| b.id.cmp(&a.id)); // ids are time-ordered
    let mut keys: Vec<&str> = vec![];
    for r in hits {
        let k = r.key.as_deref().unwrap_or("");
        if !k.is_empty() && !keys.contains(&k) {
            keys.push(k);
        }
        if keys.len() >= MAX {
            break;
        }
    }
    if keys.is_empty() {
        return vec![];
    }
    vec![format!("existing keys on these files: {}", keys.join(", "))]
}
