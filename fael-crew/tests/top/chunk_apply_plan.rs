//! `fael chunk apply --plan P`: labels in the ops resolve in plan P like every other chunk verb
//! (issue 01M5D5FCM); without `--plan` a label keeps its error.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn fael(d: &Path, args: &[&str], stdin: Option<&str>) -> (bool, String, String) {
    let mut c = Command::new(crate::common::fael_bin())
        .args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("FAEL_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(stdin.unwrap_or("").as_bytes())
        .unwrap();
    let o = c.wait_with_output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

#[test]
fn apply_resolves_labels_with_plan_and_keeps_the_error_without() {
    let d = std::env::temp_dir().join(format!("fael-apply-plan-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    // two plans share the labels, so only `--plan` says which chunks are meant
    for p in ["p1", "p2"] {
        let (ok, _, e) = fael(&d, &["plan", "new", p, "A plan"], None);
        assert!(ok, "{e}");
        for l in ["core", "retire"] {
            let args = ["chunk", "add", l, "--brief", "b", "--label", l, "--plan", p];
            let (ok, _, e) = fael(&d, &args, None);
            assert!(ok, "{e}");
        }
    }
    let ops = r#"[{"op":"pair","chunks":["core","retire"]}]"#;
    let (ok, _, err) = fael(&d, &["chunk", "apply", "--json", "-"], Some(ops));
    assert!(
        !ok && err.contains("label core names more than one chunk"),
        "{err}"
    );
    let args = ["chunk", "apply", "--json", "-", "--plan", "p1"];
    let (ok, out, err) = fael(&d, &args, Some(ops));
    assert!(ok && out.starts_with("paired "), "{out}{err}");
}
