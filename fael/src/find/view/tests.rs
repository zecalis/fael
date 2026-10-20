use super::*;
use std::process::Command;

fn g(d: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// The ref map answers every branch name as `rev-parse --verify <p><b>^{commit}`
/// does, `p` being "" then `origin/`: tag before branch, a fetch with no local,
/// a tag of a tree, a hex-like name, pseudo-refs, `refs/` paths, a name with
/// no ref at all.
#[test]
fn a_branch_name_resolves_as_rev_parse_resolves_it() {
    let d = std::env::temp_dir().join(format!("fael-view-names-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    g(&d, &["init", "-q", "-b", "main"]);
    g(&d, &["commit", "-q", "--allow-empty", "-m", "one"]);
    let one = g(&d, &["rev-parse", "HEAD"]);
    g(&d, &["commit", "-q", "--allow-empty", "-m", "two"]);
    for b in ["a", "dup", "cafe", "feat/x", "HEADLINE"] {
        g(&d, &["branch", b, "HEAD~1"]);
    }
    g(&d, &["tag", "-a", "-m", "m", "a", "HEAD"]);
    g(&d, &["tag", "dup", "HEAD"]);
    g(&d, &["tag", "-a", "-m", "m", "rel", "HEAD~1"]);
    g(&d, &["tag", "v1.0", "HEAD"]);
    g(&d, &["tag", "treetag", "HEAD^{tree}"]);
    // a tag of a tag
    g(&d, &["tag", "-a", "-m", "m", "nested", "rel"]);
    for r in [
        "refs/remotes/origin/r",
        "refs/remotes/origin/a",
        "refs/stash",
    ] {
        g(&d, &["update-ref", r, &one]);
    }
    std::fs::write(
        d.join(".git/FETCH_HEAD"),
        format!("{one}\t\tbranch 'x' of y\n"),
    )
    .unwrap();
    let view = View::load(&d);
    for b in [
        "main",
        "a",
        "dup",
        "r",
        "rel",
        "v1.0",
        "treetag",
        "nested",
        "cafe",
        "feat/x",
        "HEADLINE",
        "nope",
        "FETCH_HEAD",
        "HEAD",
        "origin/r",
        "heads/a",
        "refs/heads/a",
        "stash",
        "feat",
        "a..b",
        "",
    ] {
        let want = ["", "origin/"].iter().find_map(|p| {
            let o = Command::new("git")
                .args(["rev-parse", "--verify", "-q", &format!("{p}{b}^{{commit}}")])
                .current_dir(&d)
                .output()
                .unwrap();
            o.status
                .success()
                .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });
        assert_eq!(view.live_tip(b), want, "branch name {b:?}");
    }
}
