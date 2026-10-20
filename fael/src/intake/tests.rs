use super::*;

fn src() -> Source {
    sources(r#"{"s": {"list": "l", "draft": "d", "claim": "c"}}"#)
        .unwrap()
        .remove(0)
}

fn t(r: &str, reopen: u32) -> Listed {
    Listed {
        r#ref: r.into(),
        kind: "bug".into(),
        title: "t".into(),
        reopen,
        closed: None,
    }
}

#[test]
fn a_source_needs_a_list_and_tolerates_the_apps_other_commands() {
    assert_eq!(src().name, "s");
    assert!(sources(r#"{"s": {"read": "r"}}"#).is_err());
}

#[test]
fn a_list_line_has_exactly_the_five_fields() {
    let ok = r#"{"ref":"V-1","kind":"bug","title":"t","reopen":0,"closed":null}"#;
    assert_eq!(listed(ok).unwrap(), [t("V-1", 0)]);
    for bad in [
        r#"{"ref":"V-1","kind":"bug","title":"t","reopen":0,"closed":null,"body":"x"}"#,
        r#"{"ref":"V-1","kind":"bug","title":"t","reopen":0}"#,
        r#"{"ref":"V 1","kind":"bug","title":"t","reopen":0,"closed":null}"#,
        r#"{"ref":"V-1","kind":"bug","title":"t","reopen":-1,"closed":null}"#,
        r#"{"ref":"V-1","kind":"bug","title":" ","reopen":0,"closed":null}"#,
        r#"{"ref":"V-1","kind":"bug","title":"t","reopen":0,"closed":"dup:"}"#,
    ] {
        assert!(listed(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_key_is_lowercase_and_a_reopen_adds_the_round() {
    assert_eq!(key(&src(), &t("V-1", 0)), "ticket:s:v-1");
    assert_eq!(key(&src(), &t("V-1", 2)), "ticket:s:v-1-r2");
}

#[test]
fn a_ref_the_key_cannot_hold_stops_the_run() {
    let log = core::Log::default();
    let runs = [(src(), vec![t("V-1", 0), t("V/2", 0)])];
    let e = plan(&log, &runs).unwrap_err();
    assert!(e.contains("key"), "{e}");
}

#[test]
fn a_ticket_listed_twice_in_one_run_is_one_add() {
    let log = core::Log::default();
    let runs = [(src(), vec![t("V-1", 0), t("v-1", 0)])];
    assert_eq!(plan(&log, &runs).unwrap().len(), 1);
}
