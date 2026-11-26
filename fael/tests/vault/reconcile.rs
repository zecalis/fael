//! Reconcile after the write-path outsiders (chunk reconcile): `sync`,
//! `import`, `purge`, `restore` and `bump` leave the vault equal to a
//! `rebuild` — no manual rebuild in between.

use super::*;

/// The vault after a command holds exactly what `rebuild` would write — the
/// reconcile invariant every seam below locks in.
fn assert_matches_rebuild(d: &Path, root: &Path) {
    let before = snapshot(root);
    let (ok, _, err) = fael(d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    assert_eq!(snapshot(root), before, "command left the vault != rebuild");
}

fn git(d: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(d)
            .status()
            .unwrap()
            .success(),
        "git {args:?}"
    );
}

/// A committed repo with `fael.vault` pointed at a temp dir — sync needs a
/// commit for the repo-id; returns `(repo dir, vault repo dir)`.
fn sync_repo(name: &str, user: &str, email: &str) -> (PathBuf, PathBuf) {
    let d = std::env::temp_dir().join(format!("fael-vault-{name}-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "fn a() {}\n").unwrap();
    git(&d, &["init", "-q"]);
    git(&d, &["config", "user.name", user]);
    git(&d, &["config", "user.email", email]);
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    let vault = d.join("vault");
    git(&d, &["config", "fael.vault", vault.to_str().unwrap()]);
    let repo_name = d.file_name().unwrap().to_string_lossy().into_owned();
    (d, vault.join(repo_name))
}

#[test]
fn sync_ingest_projects_the_row_and_sync_close_removes_it() {
    let remote = std::env::temp_dir().join(format!("fael-vault-remote-{}", fael_core::ulid()));
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare"]);
    let (a, _) = sync_repo("sync-a", "Sync A", "a@example.com");
    let b_dir = std::env::temp_dir().join(format!("fael-vault-sync-b-{}", fael_core::ulid()));
    git(
        &std::env::temp_dir(),
        &["clone", "-q", a.to_str().unwrap(), b_dir.to_str().unwrap()],
    );
    git(&b_dir, &["config", "user.name", "Sync B"]);
    git(&b_dir, &["config", "user.email", "b@example.com"]);
    let vault_b = b_dir.join("vault");
    git(&b_dir, &["config", "fael.vault", vault_b.to_str().unwrap()]);
    let b_name = b_dir.file_name().unwrap().to_string_lossy().into_owned();
    let b_root = vault_b.join(b_name);
    for d in [&a, &b_dir] {
        git(d, &["config", "fael.remote", remote.to_str().unwrap()]);
    }
    // the vault starts on first `rebuild` — sync only keeps an existing one
    let (ok, _, err) = fael(&b_dir, &["vault", "rebuild"], "");
    assert!(ok, "{err}");

    let id = add_note(&a, "a row from another writer", "Synced row");
    let (ok, _, err) = fael(&a, &["sync"], "");
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&b_dir, &["sync"], "");
    assert!(ok, "{err}");
    // no manual rebuild: the sync itself projected the ingested row
    assert!(file_for(&b_root, &id).is_some(), "{:?}", md_files(&b_root));
    assert_matches_rebuild(&b_dir, &b_root);

    let (ok, _, err) = fael(&a, &["close", &id, "no longer needed"], "");
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&a, &["sync"], "");
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&b_dir, &["sync"], "");
    assert!(ok, "{err}");
    // the synced close removed the file, same as rebuild
    assert!(file_for(&b_root, &id).is_none(), "{:?}", md_files(&b_root));
    assert_matches_rebuild(&b_dir, &b_root);
    let _ = std::fs::remove_dir_all(&remote);
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b_dir);
}

#[test]
fn import_reconciles_the_vault() {
    let (d, root) = repo();
    add_note(&d, "local row", "Local row");
    let (ok, _, err) = fael(&d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    assert_eq!(md_files(&root).len(), 1);
    let log = d.join("other").join(".fael").join("log");
    std::fs::create_dir_all(&log).unwrap();
    std::fs::write(
        log.join("x.jsonl"),
        "{\"v\":1,\"ts\":\"2026-01-01T00:00:00Z\",\"by\":\"w\",\"id\":\"01AAAAAAAAAAAAAAAAAAAAAAA1\",\"kind\":\"note\",\"text\":\"imported one\",\"files\":[\"src/a.rs\"]}\n\
         {\"v\":1,\"ts\":\"2026-01-02T00:00:00Z\",\"by\":\"w\",\"id\":\"01AAAAAAAAAAAAAAAAAAAAAAA2\",\"kind\":\"note\",\"text\":\"imported two\",\"files\":[\"src/a.rs\"]}\n",
    )
    .unwrap();
    let (ok, _, err) = fael(&d, &["import", "other"], "");
    assert!(ok, "{err}");
    assert_eq!(md_files(&root).len(), 3, "{:?}", md_files(&root));
    assert_matches_rebuild(&d, &root);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn purge_reconciles_the_vault() {
    let (d, root) = repo();
    add_note(&d, "keep me", "Keep me");
    let gone = add_note(&d, "purge me", "Purge me");
    let (ok, _, err) = fael(&d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    assert_eq!(md_files(&root).len(), 2);
    let (ok, _, err) = fael(&d, &["purge", &gone], "");
    assert!(ok, "{err}");
    assert!(file_for(&root, &gone).is_none(), "{:?}", md_files(&root));
    assert_matches_rebuild(&d, &root);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn restore_reconciles_the_vault() {
    let (d, root) = repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "old bug text",
            "--title",
            "Old bug",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    let old = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "new bug text",
            "--title",
            "Old bug",
            "--files",
            "src/a.rs",
            "--supersedes",
            &old,
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(file_for(&root, &old).is_none(), "{:?}", md_files(&root));
    let (ok, _, err) = fael(&d, &["restore", &old], "");
    assert!(ok, "{err}");
    // the reopened row is projected again without a manual rebuild
    assert!(file_for(&root, &old).is_some(), "{:?}", md_files(&root));
    assert_matches_rebuild(&d, &root);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn bump_reconciles_the_vault() {
    let (d, root) = repo();
    let id = add_note(&d, "still true", "Bumped row");
    let me = file_for(&root, &id).expect("add projected the row");
    // a stale vault (file lost) comes back with the bump alone
    std::fs::remove_file(&me).unwrap();
    let (ok, _, err) = fael(&d, &["bump", &id], "");
    assert!(ok, "{err}");
    assert!(file_for(&root, &id).is_some(), "{:?}", md_files(&root));
    assert_matches_rebuild(&d, &root);
    let _ = std::fs::remove_dir_all(&d);
}
