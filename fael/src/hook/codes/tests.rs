use super::{gate_rules, reject, warning};
use std::path::Path;

#[test]
fn rejects_read_as_their_family() {
    for (msg, want) in [
        (
            "rejected: unknown flag --stale — try 'fael --help'",
            "unknown_flag",
        ),
        (
            "rejected: bump takes no --title — try 'fael --help'",
            "flag_not_taken",
        ),
        (
            "rejected: \"-k\" is not an id — copy it from fael find",
            "id_not_shaped",
        ),
        (
            "rejected: \"a\" is a path, not an id — several --files go comma-joined: --files a,b",
            "id_is_path",
        ),
        (
            "rejected: 2 of 2 ids not shown — the rest printed",
            "id_partial",
        ),
        (
            "rejected: no row with id \"01ZZ\" — copy the id from fael find",
            "id_missing",
        ),
        (
            "rejected: id \"ab\" matches 2 rows (\"01AA\", \"01AB\") — use more characters",
            "id_ambiguous",
        ),
        (
            "rejected: unknown command \"decision\" — try 'fael --help'",
            "unknown_command",
        ),
        (
            "rejected: --limit 0 shows nothing — drop it or give 1 or more",
            "bad_format",
        ),
        (
            "rejected: no chunk b9 in plan p — `fael plan export p` lists its uids",
            "chunk_missing",
        ),
        (
            "rejected: chunk 01M4AAAA is not on the board",
            "chunk_missing",
        ),
        (
            "rejected: no chunk b9 — copy a uid from `fael board`, or a label with `--plan <plan>`",
            "chunk_missing",
        ),
        (
            "rejected: chunk 01M4AAAA is not on the board — copy a live uid from `fael board`",
            "chunk_missing",
        ),
        (
            "rejected: uid prefix b9 is too short — use 8+ chars of the uid",
            "chunk_prefix",
        ),
        (
            "rejected: label foo names more than one chunk — use the uid",
            "chunk_ambiguous",
        ),
        (
            "rejected: chunk 01M4AAAA is in plan p, not q",
            "chunk_other_plan",
        ),
        (
            "rejected: chunk 01M4AAAA is in another plan",
            "chunk_other_plan",
        ),
        (
            "rejected: chunk 01M4AAAA is done — history, not edited",
            "chunk_state",
        ),
        ("rejected: chunk move needs --plan <p>", "missing_value"),
        (
            "rejected: no plan 'x' in crew.db — `fael board` lists them",
            "no_plan",
        ),
        (
            "rejected: plan p is parked — `fael plan unpark p` first",
            "plan_state",
        ),
        (
            "rejected: files is required — name the file(s) this is about",
            "missing_value",
        ),
        ("rejected: 01M4AAAA is already closed", "row_state"),
        (
            "rejected: nothing written — text is 131 words with no title — add --title \"<h>\" — try `fael add note \"<one topic>\" --title \"<≤15-word headline>\" --files <paths>`",
            "shape_gate",
        ),
    ] {
        assert_eq!(reject(msg), want, "{msg}");
    }
}

#[test]
fn a_gate_names_each_rule_once() {
    let m = "rejected: nothing written — text has 4 topic separators (; / ·) — one topic per row · \
text is 131 words with no title — add --title · decision has no --key — add --key — try `fael add decision \"<one topic>\" --files a`";
    assert_eq!(gate_rules(m), ["topic_list", "untitled", "no_key"]);
    assert!(gate_rules("rejected: nothing written for 2 of 3 rows — the rest saved").is_empty());
}

/// Every `"rejected: …"` / `"warning: …"` literal in the source, as written.
fn literals(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            if name != "tests" && name != "codes" {
                literals(&p, out);
            }
            continue;
        }
        if !name.ends_with(".rs") || name.ends_with("tests.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&p).unwrap();
        for start in ["\"rejected: ", "\"warning: "] {
            for (i, _) in src.match_indices(start) {
                let body = &src[i + 1..];
                let mut end = 0;
                let mut esc = false;
                for (j, c) in body.char_indices() {
                    match c {
                        '\\' if !esc => esc = true,
                        '"' if !esc => {
                            end = j;
                            break;
                        }
                        _ => esc = false,
                    }
                }
                out.push((p.display().to_string(), body[..end].to_string()));
            }
        }
    }
}

/// A new message gets its code in the change that adds it — else stats file
/// it under `other` and nobody can tell what to fix.
#[test]
fn every_message_in_the_source_has_a_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut all = vec![];
    literals(&root.join("src"), &mut all);
    literals(&root.join("../fael-core/src"), &mut all);
    // fael-crew's CLI rejects reach the usage log through `crew::dispatch`
    literals(&root.join("../fael-crew/src/cli"), &mut all);
    assert!(all.len() > 150, "found only {}", all.len());
    let other: Vec<_> = all
        .iter()
        .filter(|(_, m)| {
            // a wrapper around another message (`row {i}: {}`), or values
            // and one word (`{name} is {state}`): nothing a needle can hold
            let rest = m.split_once(": ").map_or("", |(_, r)| r);
            let mut words = 0;
            let mut depth = 0;
            for w in rest.split(['{', '}']) {
                if depth == 0 {
                    words += w
                        .split_whitespace()
                        .filter(|t| t.chars().any(char::is_alphabetic))
                        .count();
                }
                depth = 1 - depth;
            }
            words > 1
        })
        .filter(|(_, m)| {
            let c = if m.starts_with("warning:") {
                warning(m)
            } else {
                reject(m)
            };
            c == "other"
        })
        .collect();
    assert!(
        other.is_empty(),
        "add a needle to hook/codes.rs for:\n{other:#?}"
    );
}
