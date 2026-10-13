use super::*;

fn src(claim: &str) -> Source {
    let json = format!(
        r#"{{"s": {{"list": "l", "read": "r {{ref}}", "draft": "d {{ref}}", "open": "o/{{ref}}"{claim}}}}}"#
    );
    sources(&json).unwrap().remove(0)
}

fn t(kind: &str, reopen: u32) -> Listed {
    Listed {
        r#ref: "V-1".into(),
        kind: kind.into(),
        title: "t".into(),
        reopen,
        closed: None,
    }
}

#[test]
fn a_claim_string_claims_every_kind_but_a_question() {
    let s = src(r#", "claim": "c {ref}""#);
    assert!(brief(&s, &t("idea", 0), None).contains("\nClaim: c V-1\n"));
    assert!(!brief(&s, &t("question", 0), None).contains("Claim:"));
    assert!(!brief(&src(""), &t("bug", 0), None).contains("Claim:"));
    assert!(
        sources(r#"{"s": {"list": "l"}}"#).is_err(),
        "read, draft, open are required"
    );
}

#[test]
fn the_db_holds_one_chunk_per_ticket_round() {
    let mut s = Store::open_in_memory().unwrap();
    let src = src("");
    let runs = [(src, vec![t("bug", 0), t("bug", 1)])];
    assert_eq!(s.intake(&runs, "2026-10-13T00:00:00Z").unwrap().len(), 2);
    // the unique index is the backstop a race would hit
    let dup = s.conn.execute(
        "UPDATE chunk SET ticket_reopen = 0 WHERE label = 'v-1-r1'",
        [],
    );
    assert!(dup.is_err(), "{dup:?}");
}

#[test]
fn a_close_is_written_by_intake_not_the_owner() {
    let mut s = Store::open_in_memory().unwrap();
    let (a, b) = (
        t("bug", 0),
        Listed {
            r#ref: "V-2".into(),
            ..t("bug", 0)
        },
    );
    let open = |c: &Listed| Listed {
        closed: Some("closed".into()),
        ..c.clone()
    };
    let now = "2026-10-13T00:00:00Z";
    s.intake(&[(src(""), vec![a.clone(), b.clone()])], now)
        .unwrap();
    s.conn
        .execute("UPDATE chunk SET state = 'review' WHERE label = 'v-2'", [])
        .unwrap();
    let out = s
        .intake(&[(src(""), vec![open(&a), open(&b)])], now)
        .unwrap();
    assert_eq!(out.len(), 2, "{out:?}");
    let by = |label: &str| -> Vec<(String, String, String)> {
        s.conn
            .prepare(
                "SELECT by, to_state, text FROM event WHERE chunk_uid =
                 (SELECT uid FROM chunk WHERE label = ?1) ORDER BY id DESC LIMIT 1",
            )
            .unwrap()
            .query_map([label], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let w = |to: &str| {
        (
            "intake".to_string(),
            to.to_string(),
            "closed at source: closed".to_string(),
        )
    };
    assert_eq!(by("v-1"), [w("dropped")]);
    assert_eq!(by("v-2"), [w("done")]);
}
