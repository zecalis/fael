//! Session-start vault sync: inbox notes become rows silently, dormancy drops
//! its files, no vault means no cost and no side effects.

use super::{fael, json, repo};

fn start(d: &std::path::Path, session: &str) -> String {
    let input = format!(r#"{{"cwd":{},"session_id":"{session}"}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err} {out}");
    out
}

fn vault_files(d: &std::path::Path) -> Vec<std::path::PathBuf> {
    let vault = d.join(".git/fael/vault");
    let mut out = vec![];
    let Ok(repos) = std::fs::read_dir(&vault) else {
        return out;
    };
    for r in repos.flatten() {
        let p = r.path();
        if p == vault.join("inbox") || !p.is_dir() {
            continue;
        }
        let Ok(kinds) = std::fs::read_dir(&p) else {
            continue;
        };
        for k in kinds.flatten() {
            let Ok(files) = std::fs::read_dir(k.path()) else {
                continue;
            };
            for f in files.flatten() {
                let q = f.path();
                if q.extension().is_some_and(|x| x == "md") {
                    out.push(q);
                }
            }
        }
    }
    out
}

#[test]
fn inbox_note_files_silently_and_dormancy_drops_its_file() {
    let d = repo();
    // vault exists, one inbox note waiting
    let (ok, _, err) = fael(&d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    let inbox = d.join(".git/fael/vault/inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::write(
        inbox.join("idea.md"),
        "---\nkind: note\ntitle: quiet note\n---\nfile me without a word\n",
    )
    .unwrap();
    let out = start(&d, "s1");
    // filed as a from:user row, inbox cleared — and the hook said nothing new
    assert!(!inbox.join("idea.md").exists());
    let (_, found, _) = fael(&d, &["find", "quiet note"], "");
    assert!(found.contains("(from user)"), "{found}");
    assert!(!out.contains("quiet note"), "{out}");
    assert!(!out.contains("vault:"), "{out}");
    assert_eq!(vault_files(&d).len(), 1);

    // a note on a file that then changes goes dormant — its vault file goes too
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "tracked note", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["vault", "rebuild"], "");
    assert!(ok, "{err}");
    assert_eq!(vault_files(&d).len(), 2);
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let out = start(&d, "s2");
    assert!(!out.contains("vault:"), "{out}");
    let files = vault_files(&d);
    assert_eq!(files.len(), 1, "{files:?}");
    let body = std::fs::read_to_string(&files[0]).unwrap();
    assert!(body.contains("file me without a word"), "{body}");
}

#[test]
fn no_vault_dir_means_no_cost_no_side_effects() {
    let d = repo();
    assert!(!d.join(".git/fael/vault").exists());
    let out = start(&d, "s1");
    assert!(!out.contains("vault:"), "{out}");
    assert!(!d.join(".git/fael/vault").exists());
}
