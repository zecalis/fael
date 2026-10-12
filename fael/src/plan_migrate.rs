//! `fael plan migrate [--dry-run]` (plan fael-v8): every plan's md and every
//! `<app>/.fapony/spec/SPEC-*.md` into sections, once per repo; deletes no file.

use crate::Repo;
use fael_core::plan::{Spec, Store};
use std::path::{Path, PathBuf};

pub(super) fn run(r: &Repo, mut s: Store, dry: bool) -> Result<(), String> {
    let mut specs = Vec::new();
    for app in apps(&r.root) {
        let Ok(rd) = std::fs::read_dir(r.root.join(&app).join(".fapony/spec")) else {
            continue;
        };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if !(name.starts_with("SPEC-") && name.ends_with(".md")) {
                continue;
            }
            let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            specs.push(Spec {
                app: slash(&app),
                path: slash(f.strip_prefix(&r.root).unwrap_or(&f)),
                text,
            });
        }
    }
    let md = |p: &str| std::fs::read_to_string(r.root.join(p)).ok();
    println!("{}", s.migrate_md(&md, &specs, dry)?);
    Ok(())
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The folders a `.fapony/` may sit in: the root, `*`, `*/*` (monorepo apps). ponytail:
/// two levels, no config — add a `plan_roots` key when a repo nests apps deeper.
pub(super) fn apps(root: &Path) -> Vec<PathBuf> {
    let mut apps = vec![PathBuf::new()];
    for d in subdirs(root) {
        let rel = PathBuf::from(d.file_name().unwrap_or_default());
        apps.extend(subdirs(&d).map(|e| rel.join(e.file_name().unwrap_or_default())));
        apps.push(rel);
    }
    apps.sort();
    apps
}

fn subdirs(d: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            !n.starts_with('.') && !matches!(n.as_ref(), "node_modules" | "target")
        })
        .map(|e| e.path())
        .filter(|p| p.is_dir())
}
