//! A flag a chunk verb accepts but its `--help` usage line never names is undiscoverable: the
//! agent reaches for a feature that exists and is told to try 'fael --help'. Every flag in
//! a verb's `a.only("chunk <verb>", &[…])` list must appear in that verb's usage.

use std::collections::BTreeMap;

/// `a.only("chunk <verb>", &["x", "y"])` literals in the crew cli sources: verb → flags.
fn allowed() -> BTreeMap<String, Vec<String>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli");
    let mut out = BTreeMap::<String, Vec<String>>::new();
    for f in std::fs::read_dir(dir).unwrap().flatten() {
        let src = std::fs::read_to_string(f.path()).unwrap_or_default();
        for call in src.split(".only(").skip(1) {
            let Some(call) = call.trim_start().strip_prefix("\"chunk ") else {
                continue;
            };
            let Some((verb, rest)) = call.split_once('"') else {
                continue;
            };
            // a `&format!(…)` or a non-literal list is not ours to read
            let Some(list) = rest.trim_start_matches([',', ' ', '\r', '\n']).strip_prefix("&[") else {
                continue;
            };
            let list = list.split(']').next().unwrap_or_default();
            let flags = list.split('"').skip(1).step_by(2).map(String::from);
            out.entry(verb.into()).or_default().extend(flags);
        }
    }
    out
}

/// (verb, flag) kept out of the usage line on purpose: decision 01M5PE234 (tier table 01M59WYN),
/// born hidden, named by the text that sends the agent there (the clash reject, the board app).
const HIDDEN: &[(&str, &str)] = &[
    ("start", "with"),
    ("start", "as"),
    ("start", "session"),
    ("accept", "merged"),
];

/// The invocation lines of `fael chunk <verb> --help`, without the example line.
fn usage(verb: &str) -> String {
    let argv = ["chunk".into(), verb.to_string(), "--help".into()];
    let text = fael_core::help::for_argv(&argv);
    text.split("\neg:").next().unwrap_or_default().to_string()
}

#[test]
fn every_flag_a_chunk_verb_takes_is_in_its_usage() {
    let all = allowed();
    assert!(
        all.contains_key("done") && all.contains_key("wait"),
        "{all:?}"
    );
    let mut missing = Vec::new();
    for (verb, flags) in &all {
        let u = usage(verb);
        // `--json` and `--plan` are global reads on every verb; the rest must be named
        let shown = |f: &&String| *f != "json" && !HIDDEN.contains(&(verb.as_str(), f.as_str()));
        for f in flags.iter().filter(shown) {
            if !u.contains(&format!("--{f}")) {
                missing.push(format!("chunk {verb}: --{f} not in `{}`", u.trim()));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
    // a HIDDEN entry no verb takes any more is stale: drop it
    for (v, f) in HIDDEN {
        let live = all.get(*v).is_some_and(|fs| fs.iter().any(|x| x == f));
        assert!(live, "HIDDEN ({v}, {f}) is in no a.only list: remove it");
    }
}

/// What `fael chunk done <uid> "x" <extra…>` says on stderr, run in a scratch dir.
fn rejected(extra: &[&str]) -> String {
    let tmp = std::env::temp_dir().join(format!("fael-usage-{}", fael_core::ulid()));
    std::fs::create_dir_all(&tmp).unwrap();
    let git = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&tmp)
        .status();
    assert!(git.unwrap().success());
    let o = std::process::Command::new(crate::common::fael_bin())
        .args(["chunk", "done", "01M582E4V1RQR033P5V00B4WVV", "x"])
        .args(extra)
        .current_dir(&tmp)
        .env("FAEL_STATE_DIR", tmp.join("state"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(!o.status.success());
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A word an agent used for a feature that exists is answered with the real one and the verb's
/// usage — never the dead-end `try 'fael --help'` (it lists no chunk verb).
#[test]
fn a_wrong_word_on_chunk_done_names_the_real_one() {
    for (extra, want) in [
        (&["--verdict", "pass"][..], "--review clean|fixed|failed"),
        (&["--amend"], "amends it"),
        (&["--file", "f.md"], "reads it from stdin"),
        (&["--stdin"], "reads it from stdin"),
        (&["--bogus"], "try 'fael chunk done --help'"),
        (&["--json"], "try 'fael chunk done --help'"),
    ] {
        let err = rejected(extra);
        assert!(err.contains(want), "{extra:?}: {err}");
        assert!(!err.contains("try 'fael --help'"), "{extra:?}: {err}");
        assert!(
            err.contains("usage: fael chunk done <uid>"),
            "{extra:?}: {err}"
        );
    }
}
