//! `fael plan import` / `next` / `export` / `cutover` / `park` through the real binary: plans.db lands in the
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
    // a re-import of unchanged files keeps every uid
    let (_, before, _) = fael(&d, &["plan", "export", "root"]);
    assert!(fael(&d, &["plan", "import"]).0);
    let (ok, out, err) = fael(&d, &["plan", "export", "root"]);
    assert!(
        ok && out == before && out.contains("- [ ] r2 — next one\n  uid "),
        "{err}{out}"
    );
    let (ok, out, _) = fael(&d, &["plan", "export", "apps/vela/vela-x"]);
    assert!(ok && out.contains("· waiting · wait owner"), "{out}");
    let (ok, _, err) = fael(&d, &["plan", "export", "nope"]);
    assert!(!ok && err.contains("no plan 'nope'"), "{err}");
    let (ok, _, err) = fael(&d, &["plan", "sweep"]);
    assert!(
        !ok && err.contains("fael plan import | fael plan next | fael plan export"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn cutover_hands_the_chunks_to_the_db_and_the_brief_still_reads_the_md() {
    let d = repo();
    let md = d.join(".fapony/plan/PLAN-c.md");
    write(
        md.clone(),
        "# PLAN-c\n\n## TL;DR\n- [x] c1 — done\n- [ ] c2 — next one\n\n## 1. Goal\nship it\n",
    );
    let (ok, out, err) = fael(&d, &["plan", "cutover", "c"]);
    assert!(
        ok && out.contains("c cut over: its 2 chunks live in plans.db"),
        "{err}{out}"
    );
    let mirror = |tail: &str| {
        let text = std::fs::read_to_string(&md).unwrap();
        let want = format!(
            "- chunks live in fael — `fael board`, `fael plan export c` (mirror: edits here change nothing)\n- [x] c1 — done\n{tail}\n\n## 1. Goal"
        );
        assert!(text.contains(&want), "{text}");
    };
    mirror("- [ ] c2 — next one");
    let (ok, _, err) = fael(&d, &["plan", "cutover", "c"]);
    assert!(!ok && err.contains("already cut over"), "{err}");
    // the db owns c2 now: a chunk command takes it, the brief reads the md's Goal live
    let (_, out, _) = fael(&d, &["plan", "export", "c"]);
    let uid = out.split("- [ ] c2 — next one\n  uid ").nth(1).unwrap();
    let uid = uid.split_whitespace().next().unwrap();
    let (ok, out, err) = fael(&d, &["chunk", "start", uid]);
    assert!(ok && out.contains("## 1. Goal\nship it"), "{err}{out}");
    // every chunk command rewrites the mirror; push pr is the ok, so done --pr is done
    mirror("- [ ] c2 — next one · **running**");
    let (ok, out, err) = fael(&d, &["chunk", "done", uid, "shipped", "--pr", "7"]);
    assert!(ok && out.contains("→ done"), "{err}{out}");
    mirror("- [x] c2 — next one");
    assert!(fael(&d, &["plan", "import"]).0, "a mirrored md re-imports");
    let (_, out, _) = fael(&d, &["plan", "export", "c"]);
    assert!(out.contains("- [x] c2 — next one\n  uid"), "{out}");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_refused_cutover_leaves_the_md_and_an_app_plan_cuts_over_by_app_name() {
    let d = repo();
    let bad = "# PLAN-x\n\n## TL;DR\n- [ ] x1 (after nope) — waits on nothing\n";
    let root = d.join(".fapony/plan/PLAN-x.md");
    write(root.clone(), bad);
    // refused: the db keeps truth md and the file is byte for byte the same
    let (ok, _, err) = fael(&d, &["plan", "cutover", "x"]);
    assert!(
        !ok && err.contains("x1: (after nope) names no chunk"),
        "{err}"
    );
    assert_eq!(std::fs::read_to_string(&root).unwrap(), bad);
    let (_, out, _) = fael(&d, &["plan", "export", "x"]);
    assert!(out.contains("· truth md ·"), "{out}");
    // the same name under apps/: the bare name is ambiguous, app/name cuts that file only
    let app = d.join("apps/vela/.fapony/plan/PLAN-x.md");
    write(app.clone(), "# PLAN-x\n\n## TL;DR\n- [ ] v1 — one\n");
    let (ok, _, err) = fael(&d, &["plan", "cutover", "x"]);
    assert!(!ok && err.contains("names 2 plans — use app/name"), "{err}");
    let (ok, out, err) = fael(&d, &["plan", "cutover", "apps/vela/x"]);
    assert!(
        ok && out.contains("apps/vela/x cut over: its 1 chunks"),
        "{err}{out}"
    );
    let text = std::fs::read_to_string(&app).unwrap();
    assert!(text.contains("`fael plan export apps/vela/x`"), "{text}");
    assert_eq!(std::fs::read_to_string(&root).unwrap(), bad);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_name_in_two_folders_is_refused_and_names_both_files() {
    let d = repo();
    let live = "# PLAN-s\n\n## TL;DR\n- [ ] s1 — new\n";
    write(d.join(".fapony/plan/PLAN-s.md"), live);
    write(
        d.join(".fapony/done/PLAN-s.md"),
        "# PLAN-s\n\n## TL;DR\n- [x] s1 — shipped\n",
    );
    let (ok, _, err) = fael(&d, &["plan", "cutover", "s"]);
    assert!(
        !ok && err.contains(".fapony/done/PLAN-s.md, .fapony/plan/PLAN-s.md"),
        "{err}"
    );
    assert_eq!(
        std::fs::read_to_string(d.join(".fapony/plan/PLAN-s.md")).unwrap(),
        live
    );
    // one match left: it cuts over
    std::fs::remove_file(d.join(".fapony/done/PLAN-s.md")).unwrap();
    let (ok, out, err) = fael(&d, &["plan", "cutover", "s"]);
    assert!(ok && out.contains("s cut over: its 1 chunks"), "{err}{out}");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn park_unpark_done_move_a_cut_over_plan_and_a_reimport_keeps_it() {
    let d = repo();
    write(
        d.join(".fapony/plan/PLAN-p.md"),
        "# PLAN-p\n\n## TL;DR\n- [ ] p1 — one\n",
    );
    write(
        d.join(".fapony/plan/PLAN-m.md"),
        "# PLAN-m\n\n## TL;DR\n- [ ] m1 — one\n",
    );
    assert!(fael(&d, &["plan", "cutover", "p"]).0);
    // an md plan's folder is its state
    let (ok, _, err) = fael(&d, &["plan", "park", "m"]);
    assert!(
        !ok && err.contains("move the file to .fapony/parked/"),
        "{err}"
    );
    let (ok, out, err) = fael(&d, &["plan", "park", "p"]);
    assert!(ok && out == "plan p active → parked\n", "{err}{out}");
    let board = || fael(&d, &["board"]).1;
    assert!(!board().contains("p p1"), "{}", board());
    // the md still sits in plan/: a re-import leaves the db plan parked
    assert!(fael(&d, &["plan", "import"]).0);
    assert!(fael(&d, &["plan", "export", "p"]).1.contains("· parked ·"));
    let (ok, _, err) = fael(&d, &["plan", "park", "p"]);
    assert!(!ok && err.contains("p is parked"), "{err}");
    assert!(fael(&d, &["plan", "unpark", "p"]).0);
    assert!(board().contains("p p1"), "{}", board());
    // a running chunk keeps its plan on the board
    let (_, out, _) = fael(&d, &["plan", "export", "p"]);
    let uid = out
        .split("uid ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    assert!(fael(&d, &["chunk", "start", uid]).0);
    let (ok, _, err) = fael(&d, &["plan", "done", "p"]);
    assert!(!ok && err.contains("1 running chunk(s)"), "{err}");
    assert!(fael(&d, &["chunk", "done", uid, "shipped", "--pr", "1"]).0);
    let (ok, out, err) = fael(&d, &["plan", "done", "p"]);
    assert!(ok && out == "plan p active → done\n", "{err}{out}");
    let _ = std::fs::remove_dir_all(d);
}
