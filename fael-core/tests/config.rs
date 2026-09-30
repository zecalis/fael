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
fn capture_block_defaults_off_and_parses() {
    assert!(!Config::from_toml("").unwrap().capture_block);
    assert!(
        Config::from_toml("[capture]\nblock = true")
            .unwrap()
            .capture_block
    );
    // a non-bool must fail loudly, never silently keep or flip enforcement
    assert!(Config::from_toml("[capture]\nblock = \"yes\"").is_err());
}
