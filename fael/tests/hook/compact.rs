//! An edit push names no file list: one line per row
//! (`<kind>: <title60> (<id>)`), the edited file dropped, the rest one
//! `fael find <id>` away.

use super::{fael, json, repo};

#[test]
fn an_edit_push_renders_one_line_per_row_with_no_paths() {
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
    let (ok, out, err) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok, "{err}");
    let line = |t: &str| {
        out.split("\\n")
            .find(|l| l.contains(t))
            .unwrap_or_default()
            .to_string()
    };
    // one line per row, no arrow, no file list — the agent just edited src/a.rs
    for t in ["only here", "here and one more", "here and three more"] {
        let l = line(t);
        assert!(l.starts_with("- note: "), "{l}\n{out}");
        assert!(!l.contains('→'), "{l}\n{out}");
        assert!(!l.contains("src/"), "{l}\n{out}");
    }
}
