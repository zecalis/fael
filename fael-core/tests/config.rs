//! `.fael/config.toml` parsing — every field optional, defaults hold.

use fael_core::*;

#[test]
fn anchor_prefixes_default_and_parse() {
    let d = Config::from_toml("").unwrap();
    assert_eq!(d.anchor_prefixes, vec!["PLAN-".to_string()]);
    let c = Config::from_toml("[anchor]\nprefixes = [\"HANDOFF-\", \"brief-\"]").unwrap();
    assert_eq!(
        c.anchor_prefixes,
        vec!["HANDOFF-".to_string(), "brief-".to_string()]
    );
    // a config that says nothing about anchors keeps the default
    let c = Config::from_toml("[budget]\nsession_decisions = 1").unwrap();
    assert_eq!(c.anchor_prefixes, vec!["PLAN-".to_string()]);
}

#[test]
fn cross_key_default_warn_parse_and_reject() {
    use fael_core::CrossKey;
    assert_eq!(Config::from_toml("").unwrap().cross_key, CrossKey::Warn);
    for (toml, want) in [
        ("[selfheal]\ncross_key = \"warn\"", CrossKey::Warn),
        ("[selfheal]\ncross_key = \"info\"", CrossKey::Info),
        ("[selfheal]\ncross_key = \"off\"", CrossKey::Off),
    ] {
        assert_eq!(Config::from_toml(toml).unwrap().cross_key, want, "{toml}");
    }
    // a typo must fail loudly, never flip an ask to silence
    let e = Config::from_toml("[selfheal]\ncross_key = \"hold\"").unwrap_err();
    assert!(e.contains("cross_key") && e.contains("warn"), "{e}");
}

#[test]
fn removed_capture_block_key_still_parses() {
    // the Stop-block mode is gone; a config that opted in must not break
    assert!(Config::from_toml("[capture]\nblock = true").is_ok());
}

#[test]
fn hint_stop_default_empty_and_parse_lowercased() {
    assert!(Config::from_toml("").unwrap().hint_stop.is_empty());
    let c = Config::from_toml("[hint]\nstop = [\"Workspace\", \" file \", \"\"]").unwrap();
    assert_eq!(c.hint_stop, ["workspace", "file"]);
}

#[test]
fn hint_stop_edges_empty_ok_wrong_type_fails_loudly() {
    // an explicit empty list and a `[hint]` table without `stop` both stop nothing
    for toml in ["[hint]\nstop = []", "[hint]"] {
        assert!(
            Config::from_toml(toml).unwrap().hint_stop.is_empty(),
            "{toml}"
        );
    }
    // a bare string is not a list: reject it, never silently stop nothing
    for toml in ["[hint]\nstop = \"workspace\"", "[hint]\nstop = [1]"] {
        let e = Config::from_toml(toml).unwrap_err();
        assert!(e.contains("stop"), "{toml}: {e}");
    }
}

#[test]
fn push_policy_unset_is_auto_and_a_pin_or_a_typo_is_judged() {
    // unset = the repo's own stages; the share only matters for a pin
    let d = Config::from_toml("").unwrap();
    assert_eq!((d.push_policy.as_str(), d.push_holdout), (AUTO, 20));
    for v in ["auto", "baseline@1", "touch@1"] {
        let c = Config::from_toml(&format!("push_policy = \"{v}\"")).unwrap();
        assert_eq!(c.push_policy, v);
    }
    // a silent typo would run (or skip) the experiment; the error names `auto`
    let e = Config::from_toml("push_policy = \"touch@9\"").unwrap_err();
    assert!(e.contains("push_policy") && e.contains("auto"), "{e}");
    let e = Config::from_toml("push_holdout = 101").unwrap_err();
    assert!(e.contains("push_holdout"), "{e}");
}
