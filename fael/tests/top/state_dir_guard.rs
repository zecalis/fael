//! Every test file that spawns the fael binary points `FAEL_STATE_DIR` at a
//! scratch dir (directly or through a `state_env` helper), so a test run never
//! writes into the developer's real usage.jsonl (fael:01M4F3G0).

use std::path::{Path, PathBuf};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn every_suite_spawning_fael_sets_a_state_dir() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for d in [root.join("tests"), root.join("../fael-core/tests")] {
        if d.is_dir() {
            rs_files(&d, &mut files);
        }
    }
    let me = file!().rsplit(['/', '\\']).next().unwrap();
    let leaks: Vec<_> = files
        .iter()
        .filter(|p| !p.ends_with(me))
        .filter(|p| {
            let s = std::fs::read_to_string(p).unwrap();
            s.contains("CARGO_BIN_EXE_fael")
                && !s.contains("FAEL_STATE_DIR")
                && !s.contains("state_env(")
        })
        .collect();
    assert!(
        leaks.is_empty(),
        "spawns fael without FAEL_STATE_DIR: {leaks:?}"
    );
}
