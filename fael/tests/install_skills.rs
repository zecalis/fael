//! `fael install` writes every skill fael ships (issue 01M4N42X), and a dev's link into a
//! fael checkout gives way to the release copy without touching that checkout.

use std::path::Path;
use std::process::Command;

fn install(home: &Path) -> String {
    install_with(home, &[])
}

fn install_with(home: &Path, extra: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["install", "--client", "claude"])
        .args(extra)
        .env("HOME", home)
        // never the developer's real usage log (fael:01M4F3G0)
        .env("FAEL_STATE_DIR", home.join("state"))
        // fael on PATH (install refuses without it), no claude CLI: MCP is printed, not run
        .env(
            "PATH",
            std::env::join_paths([
                Path::new(env!("CARGO_BIN_EXE_fael")).parent().unwrap(),
                Path::new("/usr/bin"),
                Path::new("/bin"),
            ])
            .unwrap(),
        )
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

#[test]
fn install_writes_every_shipped_skill() {
    let home = std::env::temp_dir().join(format!("fael-skills-{}", fael_core::ulid()));
    install(&home);
    for name in [
        "fael",
        "fael-content",
        "fael-debug",
        "fael-plan",
        "fael-review",
    ] {
        let s = read(&home.join(".claude/skills").join(name).join("SKILL.md"));
        // lines(), not "\n": a Windows checkout embeds the skill with CRLF
        assert!(
            s.lines().any(|l| l == format!("name: {name}")),
            "{name}: {s}"
        );
    }
    let again = install(&home);
    assert!(again.contains("up to date"), "{again}");
    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn a_link_into_a_fael_checkout_gives_way_and_a_foreign_link_stays() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!("fael-skills-{}", fael_core::ulid()));
    let home = base.join("home");
    let skills = home.join(".claude/skills");
    std::fs::create_dir_all(&skills).unwrap();
    // a worktree's skill dir, linked by hand before fael shipped it
    let checkout = base.join("wt/fael/skill/fael-content");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::write(checkout.join("SKILL.md"), "old draft\n").unwrap();
    symlink(&checkout, skills.join("fael-content")).unwrap();
    // someone else's skill under fael's name
    let other = base.join("other/fael");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("SKILL.md"), "mine\n").unwrap();
    symlink(&other, skills.join("fael")).unwrap();

    let out = install(&home);
    let dir = skills.join("fael-content");
    assert!(!dir.is_symlink() && read(&dir.join("SKILL.md")).contains("name: fael-content"));
    assert_eq!(
        read(&checkout.join("SKILL.md")),
        "old draft\n",
        "checkout untouched"
    );
    assert!(skills.join("fael").is_symlink(), "{out}");
    assert_eq!(read(&other.join("SKILL.md")), "mine\n");
    assert!(out.contains("is a link, not fael's"), "{out}");
    let _ = std::fs::remove_dir_all(&base);
}

/// A retired name linked into a fael checkout goes on every install; one linked into
/// fapony's package goes only with --replace-fapony; a real dir under it stays.
#[cfg(unix)]
#[test]
fn retired_skill_links_go_and_real_dirs_stay() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!("fael-skills-{}", fael_core::ulid()));
    let home = base.join("home");
    let skills = home.join(".claude/skills");
    std::fs::create_dir_all(skills.join("debug-pony")).unwrap();
    symlink(
        base.join("wt/fael/skill/plan-with-marketing"),
        skills.join("plan-with-marketing"),
    )
    .unwrap();
    let pkg = base.join("bun/node_modules/@zecalis/fapony/skill");
    symlink(pkg.join("review-pony"), skills.join("review-pony")).unwrap();
    symlink(pkg.join("git-ship"), skills.join("git-ship")).unwrap();

    install(&home);
    assert!(!skills.join("plan-with-marketing").is_symlink());
    assert!(
        skills.join("review-pony").is_symlink(),
        "fapony's stay without the flag"
    );
    install_with(&home, &["--replace-fapony"]);
    for gone in ["review-pony", "git-ship"] {
        assert!(!skills.join(gone).is_symlink(), "{gone}");
    }
    assert!(
        skills.join("debug-pony").is_dir(),
        "a real dir is someone's own"
    );
    let _ = std::fs::remove_dir_all(&base);
}
