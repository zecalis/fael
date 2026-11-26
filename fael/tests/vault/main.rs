//! `fael vault rebuild` + the write-path projection (chunk vault): the open
//! rows as markdown files, kept in sync by add/close/supersede.
//!
//! Thin entry like the suites beside it — every test runs the real binary in
//! a temp git repo with `fael.vault` pointed at a temp dir.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn state_env(c: &mut Command, dir: &Path) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    c.env("FAEL_STATE_DIR", root.join("state"));
}

fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(dir);
    state_env(&mut c, dir);
    c.env_remove("FAEL_SESSION");
    c.env_remove("CODEX_THREAD_ID");
    c.env_remove("CLAUDE_CODE_SESSION_ID");
    if !stdin.is_empty() {
        c.stdin(Stdio::piped());
    }
    let mut c = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if !stdin.is_empty() {
        c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    }
    let o = c.wait_with_output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// A temp git repo with `fael.vault` pointed at `<tmp>/vault`; returns
/// `(repo dir, vault repo dir)`.
fn repo() -> (PathBuf, PathBuf) {
    let d = std::env::temp_dir().join(format!("fael-vault-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "fn a() {}\n").unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Vault Test"],
        &["config", "user.email", "vault@example.com"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&d)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    let vault = d.join("vault");
    assert!(
        Command::new("git")
            .args(["config", "fael.vault"])
            .arg(&vault)
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let name = d.file_name().unwrap().to_string_lossy().into_owned();
    (d, vault.join(name))
}

fn md_files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(kinds) = std::fs::read_dir(root) else {
        return out;
    };
    for kind in kinds.flatten() {
        let Ok(files) = std::fs::read_dir(kind.path()) else {
            continue;
        };
        out.extend(
            files
                .flatten()
                .map(|f| f.path())
                .filter(|p| p.extension().is_some_and(|x| x == "md")),
        );
    }
    out.sort();
    out
}

fn open_count(d: &Path) -> usize {
    let (ok, out, err) = fael(d, &["find", "--json"], "");
    assert!(ok, "{err}");
    // an empty result is one summary line with no id — only rows count
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("id").and_then(|i| i.as_str()).is_some())
        .count()
}

/// The vault file holding row `id` (frontmatter-verified).
fn file_for(root: &Path, id: &str) -> Option<PathBuf> {
    md_files(root).into_iter().find(|p| {
        std::fs::read_to_string(p)
            .ok()
            .is_some_and(|b| b.contains(&format!("id: \"{id}\"")))
    })
}

fn add_note(d: &Path, text: &str, title: &str) -> String {
    let (ok, out, err) = fael(
        d,
        &["add", "note", text, "--title", title, "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

#[test]
fn add_creates_exactly_one_file_with_aliases() {
    let (d, root) = repo();
    let id = add_note(&d, "the vault holds open rows", "Vault holds rows");
    let files = md_files(&root);
    assert_eq!(files.len(), 1, "{files:?}");
    let body = std::fs::read_to_string(&files[0]).unwrap();
    let short = body
        .lines()
        .find_map(|l| {
            l.strip_prefix("aliases: [\"")?
                .strip_suffix("\"]")
                .map(str::to_string)
        })
        .expect("aliases entry");
    assert!(short.len() >= 8 && id.starts_with(&short), "{body}");
    assert!(files[0].to_string_lossy().ends_with(&format!("-{short}.md")));
    assert!(body.contains(&format!("id: \"{id}\"")), "{body}");
    assert!(body.contains("kind: \"note\""), "{body}");
    assert!(body.contains("the vault holds open rows"), "{body}");
    assert_eq!(open_count(&d), files.len());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn close_removes_the_file_in_the_same_call() {
    let (d, root) = repo();
    let id = add_note(&d, "short lived note", "Short lived");
    assert_eq!(md_files(&root).len(), 1);
    let (ok, _, err) = fael(&d, &["close", &id, "no longer needed"], "");
    assert!(ok, "{err}");
    assert_eq!(md_files(&root).len(), 0);
    assert_eq!(open_count(&d), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn supersede_swaps_the_file() {
    let (d, root) = repo();
    let (ok, out, err) = fael(
        &d,
        &["add", "issue", "old bug text", "--title", "Old bug", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let old = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(
        &d,
        &[
            "add", "issue", "new bug text", "--title", "Old bug",
            "--files", "src/a.rs", "--supersedes", &old,
        ],
        "",
    );
    assert!(ok, "{err}");
    let new = out.split_whitespace().next().unwrap().to_string();
    assert_ne!(old, new);
    let files = md_files(&root);
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(file_for(&root, &old).is_none(), "{files:?}");
    let kept = file_for(&root, &new).expect("new row's file");
    assert_eq!(files[0], kept);
    assert_eq!(open_count(&d), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cited_ids_become_links() {
    let (d, root) = repo();
    let first = add_note(&d, "the referenced row", "Referenced row");
    let second = add_note(&d, &format!("see also {first} for context"), "Referencing row");
    assert_ne!(first, second);
    let files = md_files(&root);
    assert_eq!(files.len(), 2, "{files:?}");
    let me = file_for(&root, &second).expect("citing row's file");
    let body = std::fs::read_to_string(&me).unwrap();
    let linked = body
        .lines()
        .filter(|l| l.contains("[[") && l.contains("]]"))
        .any(|l| first.starts_with(l.split("[[").nth(1).unwrap_or("").split("]]").next().unwrap_or("!")));
    assert!(linked, "cited id links as [[short]]: {body}");
    assert!(!body.contains(&first), "raw id must be linked, not pasted: {body}");
    let _ = std::fs::remove_dir_all(&d);
}

fn snapshot(root: &Path) -> Vec<(PathBuf, String)> {
    md_files(root)
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            let body = std::fs::read_to_string(&p).unwrap();
            (rel, body)
        })
        .collect()
}

#[test]
fn rebuild_reproduces_bytes_and_clears_stale() {
    let (d, root) = repo();
    add_note(&d, "first row", "First row");
    add_note(&d, "second row", "Second row");
    let (ok, _, err) = fael(&d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    let before = snapshot(&root);
    assert_eq!(before.len(), 2);
    // stale files: a forged projection, a foreign file, a crashed tmp
    let kind_dir = root.join("note");
    std::fs::write(kind_dir.join("forged-12345678.md"), "---\nid: \"x\"\n---\nforged\n").unwrap();
    std::fs::write(kind_dir.join("notes.txt"), "not a projection").unwrap();
    std::fs::write(kind_dir.join("crash-12345678.md.tmp"), "half written").unwrap();
    let (ok, out, err) = fael(&d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    assert!(out.contains("removed"), "{out}");
    let after = snapshot(&root);
    assert_eq!(before, after);
    assert_eq!(open_count(&d), after.len());
    // full wipe reproduces byte for byte
    std::fs::remove_dir_all(&root).unwrap();
    let (ok, _, err) = fael(&d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    assert_eq!(snapshot(&root), before);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn no_vault_dir_means_no_cost() {
    let d = std::env::temp_dir().join(format!("fael-novault-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "No Vault"],
        &["config", "user.email", "novault@example.com"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&d)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "no vault here", "--files", "doc:seed"], "");
    assert!(ok, "{err}");
    // no vault projected beside the journal, and the add still worked
    assert!(!d.join(".git/fael/vault").exists());
    assert_eq!(open_count(&d), 1);
    let _ = std::fs::remove_dir_all(&d);
}
