//! `fael plan import` / `fael plan next` through the real binary: plans.db lands in the
//! git common dir, an app under apps/ is found, and `next` without a db is a reject.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-plan-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    d
}

fn write(p: PathBuf, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn import_then_next_per_app() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["plan", "next"]);
    assert!(!ok && err.contains("fael plan import"), "{err}");
    write(
        d.join(".fapony/plan/PLAN-root.md"),
        "# PLAN-root\n\n## TL;DR\n- [x] r1 — done\n- [ ] r2 — next one\n",
    );
    write(
        d.join("apps/vela/.fapony/plan/PLAN-vela-x.md"),
        "# PLAN-vela-x\n\n## TL;DR\n- [ ] v1 (wait owner) — held\n- [?] v2 — odd\n- [ ] v3 (after nope) — blocked\n",
    );
    let (ok, out, err) = fael(&d, &["plan", "import"]);
    assert!(ok, "{err}");
    assert!(out.contains("imported 2 plans, 5 chunks"), "{out}");
    assert!(out.contains("1 unknown checkbox"), "{out}");
    assert!(out.contains("apps/vela/vela-x v3 → nope"), "{out}");
    assert!(d.join(".git/fael/plans.db").exists());
    let (ok, out, err) = fael(&d, &["plan", "next"]);
    assert!(ok, "{err}");
    assert_eq!(
        out,
        "root\t- [ ] r2 — next one\napps/vela/vela-x\t(none ready)\n"
    );
    let (ok, _, err) = fael(&d, &["plan", "sweep"]);
    assert!(
        !ok && err.contains("fael plan import | fael plan next"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(d);
}
