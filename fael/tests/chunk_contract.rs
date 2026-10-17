//! h2: `chunk start` prints each open `contract:*` row whose files overlap the chunk's
//! scope under its own `contracts:` heading — never one off the scope, never a superseded
//! one — and `chunk done` then says what left that scope (h4).

use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID").env_remove("FAEL_SESSION").env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{args:?}: {err}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    let ok = Command::new("git")
        .args(id)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn start_prints_the_contracts_on_its_scope_and_done_says_what_left_it() {
    let tmp = std::env::temp_dir().join(format!("fael-contract-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("README"), "base").unwrap();
    git(&d, &["add", "README"]);
    git(&d, &["commit", "-qm", "init"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    let head = "refs/remotes/origin/HEAD";
    git(&d, &["symbolic-ref", head, "refs/remotes/origin/main"]);
    let f = |args: &[&str]| fael(&d, &state, args);
    // the ledger id `fael add` prints first
    let add = |key: &str, file: &str, body: &str, more: &[&str]| {
        let args = [
            "add", "decision", body, "--title", key, "--key", key, "--files", file,
        ];
        let out = f(&[&args[..], more].concat());
        out.split_whitespace().next().unwrap().to_string()
    };
    let x = add("contract:p:x", "a/x.rs", "x takes a u32", &[]);
    let y = add("contract:p:y", "b/y.rs", "y stays sync", &[]);
    let old = add("contract:p:z", "a/z.rs", "z returns a Vec", &[]);
    let new = add(
        "contract:p:z2",
        "a/z.rs",
        "z returns a slice",
        &["--supersedes", &old],
    );
    let chunk = |label: &str, scope: &str| {
        let args = ["--brief", "b", "--label", label, "--scope", scope];
        f(&[&["chunk", "add", label], &args[..]].concat())
    };
    let (a, _b) = (chunk("a", "a/"), chunk("b", "b/"));

    git(&d, &["switch", "-q", "-c", "lane"]);
    let out = f(&["chunk", "start", &a, "--run", "R"]);
    let block = out
        .split_once("\ncontracts:\n")
        .map(|(_, b)| b)
        .unwrap_or("");
    assert!(
        block.contains(&format!("> {x} contract:p:x\n> x takes a u32")),
        "{out}"
    );
    assert!(block.contains(&format!("> {new} contract:p:z2")), "{out}");
    assert!(
        !out.contains(&y) && !out.contains(&old),
        "off scope or superseded: {out}"
    );

    for p in ["a/x.rs", "c/new.rs", "b/y.rs"] {
        std::fs::create_dir_all(d.join(p).parent().unwrap()).unwrap();
        std::fs::write(d.join(p), p).unwrap();
        git(&d, &["add", p]);
    }
    git(&d, &["commit", "-qm", "lane"]);
    let out = f(&["chunk", "done", &a, "h", "--run", "R"]);
    let block = out.lines().skip(1).collect::<Vec<_>>().join("\n");
    let want = "declared scope: a/\noutside, in inbox/b: b/y.rs\noutside, unowned: c/new.rs\n";
    // the held lines over the touched files follow (plan fael-row-retire w2)
    assert!(block.starts_with(want), "{block}");

    // a chunk whose scope holds no contract prints no heading
    let c = chunk("c", "c/");
    let out = f(&["chunk", "start", &c, "--run", "R2"]);
    assert!(!out.contains("contracts:"), "{out}");
}
