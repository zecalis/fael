//! Chunk 3 (PLAN-fael-selfheal-verdict): the incident corpus. The corpus
//! catches cases we have seen; the generated property tests catch cases we
//! have not.
//!
//! Each `*.json` next to this file is one incident: seed rows, the add that
//! replays it, and what the CLI must show. Fixture keys stay
//! levenshtein-distant (`auth:session` vs `billing:invoice`): close keys
//! would trip core's similar-key warning and the ask count would stop
//! proving anything about the self-heal line.

use super::{fael, repo, usage};

/// One `add` from fixture data: `{kind, text, files[], key?}`.
fn add_row(d: &std::path::Path, v: &serde_json::Value) -> (bool, String, String) {
    let kind = v["kind"].as_str().unwrap().to_string();
    let text = v["text"].as_str().unwrap().to_string();
    let files = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(",");
    let mut args = vec![
        "add",
        kind.as_str(),
        text.as_str(),
        "--files",
        files.as_str(),
    ];
    let key;
    if let Some(k) = v["key"].as_str() {
        key = k.to_string();
        args.extend(["--key", key.as_str()]);
    }
    fael(d, &args, "")
}

/// Usage rows with `ask == "warning"` — the only asks a self-heal line may be.
fn warnings(d: &std::path::Path) -> Vec<serde_json::Value> {
    usage(d)
        .into_iter()
        .filter(|u| u["ask"] == "warning")
        .collect()
}

fn replay(path: &std::path::Path) {
    let raw = std::fs::read_to_string(path).unwrap();
    let fx: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let name = fx["name"].as_str().unwrap_or("?");
    let tag = format!("{} ({})", path.display(), name);
    let d = repo();
    if let Some(cfg) = fx["config"].as_str() {
        std::fs::create_dir_all(d.join(".fael")).unwrap();
        std::fs::write(d.join(".fael/config.toml"), cfg).unwrap();
    }
    let mut ids = vec![];
    for s in fx["seed"].as_array().unwrap() {
        let (ok, out, err) = add_row(&d, s);
        assert!(ok, "{tag}: seed failed: {err}");
        ids.push(out.split_whitespace().next().unwrap().to_string());
    }
    // seeds file silently: no warning may precede the replayed add
    assert!(warnings(&d).is_empty(), "{tag}: a seed warned");
    let (ok, _, err) = add_row(&d, &fx["add"]);
    let ex = &fx["expect"];
    assert!(ok, "{tag}: {err}");
    let warns: Vec<&str> = err.lines().filter(|l| l.starts_with("warning:")).collect();
    assert_eq!(
        warns.len(),
        ex["warning_lines"].as_u64().unwrap() as usize,
        "{tag}: {err}"
    );
    for w in ex["warning_contains"].as_array().unwrap_or(&vec![]) {
        assert!(err.contains(w.as_str().unwrap()), "{tag}: {err}");
    }
    for w in ex["info_contains"].as_array().unwrap_or(&vec![]) {
        assert!(err.contains(w.as_str().unwrap()), "{tag}: {err}");
    }
    if ex["stderr_empty"].as_bool().unwrap_or(false) {
        assert!(err.trim().is_empty(), "{tag}: {err}");
    }
    let u = warnings(&d);
    assert_eq!(
        u.len(),
        ex["usage_warnings"].as_u64().unwrap() as usize,
        "{tag}: {u:?}"
    );
    let (ok, rows, err) = fael(&d, &["find", "--json", "--all"], "");
    assert!(ok, "{tag}: {err}");
    let want_text = fx["add"]["text"].as_str().unwrap();
    let newest = rows
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["text"].as_str() == Some(want_text))
        .unwrap();
    // `null`: the row is filed and the seed stays open
    let want = ex["supersedes_seed"]
        .as_u64()
        .map(|i| ids[i as usize].as_str());
    assert_eq!(newest["supersedes"].as_str(), want, "{tag}: {newest}");
    assert_eq!(
        newest["decision_source"], ex["decision_source"],
        "{tag}: {newest}"
    );
}

#[test]
fn corpus_replays_incidents() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/selfheal/corpus");
    let mut fxs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    fxs.sort();
    assert!(!fxs.is_empty(), "no corpus fixtures in {}", dir.display());
    for f in &fxs {
        replay(f);
    }
}
