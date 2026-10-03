//! A file push names only the files its row is about besides the one the agent
//! opened — it knows that one.

use super::{fael, json, repo};

#[test]
fn a_read_push_drops_the_opened_file_and_folds_the_rest() {
    let d = repo();
    for f in ["a", "b", "c", "d"] {
        std::fs::write(d.join(format!("src/{f}.rs")), "//\n").unwrap();
    }
    // one key each, or the later row supersedes the earlier on shared files
    let add = |text: &str, files: &str| {
        let key = text.replace(' ', "-");
        let args = ["add", "note", text, "--files", files, "--key", &key];
        let (ok, _, err) = fael(&d, &args, "");
        assert!(ok, "{err}");
    };
    add("only here", "src/a.rs");
    add("here and one more", "src/a.rs,src/b.rs");
    add("here and three more", "src/a.rs,src/b.rs,src/c.rs,src/d.rs");
    let input = format!(
        r#"{{"cwd":{},"tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, err) = fael(&d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok, "{err}");
    let line = |t: &str| {
        out.split("\\n")
            .find(|l| l.contains(t))
            .unwrap_or_default()
            .to_string()
    };
    assert!(!line("only here").contains('→'), "{out}");
    assert!(
        line("here and one more").ends_with("→ also: src/b.rs"),
        "{out}"
    );
    assert!(
        line("here and three more").ends_with("→ also: src/b.rs, src/c.rs +1"),
        "{out}"
    );
}
