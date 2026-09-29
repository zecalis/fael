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
