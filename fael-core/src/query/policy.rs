//! Push policy identity (SPEC-fael-learn-loop §D).

/// A push policy's identity (SPEC-fael-learn-loop §D): `<id>@<version>`. A
/// released version's definition never changes — a different rule or
/// threshold is a new version, and the golden test below breaks if one drifts.
#[derive(Debug, PartialEq, Eq)]
pub struct PolicyDef {
    pub id: &'static str,
    pub version: u32,
    pub rule: &'static str,
}

impl PolicyDef {
    /// `baseline@1` — what a usage line's `policy` carries.
    pub fn name(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}

/// Today's push, named: nothing is cut by evidence, only by the row cap, the
/// hub peek (`PUSH_HUB_ROWS` / `PUSH_HUB_PEEK`) and the token budget.
pub const BASELINE: PolicyDef = PolicyDef {
    id: "baseline",
    version: 1,
    rule: "no gate; cut by row cap, hub peek and token budget only",
};

/// Shadow candidate (chunk 3): a said row whose files the session has not
/// touched yet is the low-yield one (23% in-context at edit, against 43–49%
/// once touched). Never applied — a usage line's `would_drop` records what it
/// would have cut, so chunk 4 can replay it against the outcomes.
pub const TOUCH: PolicyDef = PolicyDef {
    id: "touch",
    version: 1,
    rule: "drop a said row when none of its files was touched earlier in the session, unless it is an issue or a handoff",
};

/// Would `touch@1` drop `r` when `touch` of its files were already in the
/// session's working set? Issues and handoffs stay: no yield data backs
/// cutting them yet.
pub fn touch_drops(r: &crate::Row, touch: usize) -> bool {
    touch == 0 && r.kind != "issue" && !r.key.as_deref().is_some_and(|k| k.ends_with(":handoff"))
}

#[cfg(test)]
mod tests {
    use super::super::{PUSH_HUB_PEEK, PUSH_HUB_ROWS};
    use super::*;

    #[test]
    fn baseline_1_is_pinned() {
        assert_eq!(BASELINE.name(), "baseline@1");
        assert_eq!(
            BASELINE.rule,
            "no gate; cut by row cap, hub peek and token budget only"
        );
        // the rule rests on these two: change either and baseline@1 is a lie —
        // release baseline@2 instead
        assert_eq!((PUSH_HUB_ROWS, PUSH_HUB_PEEK), (8, 3));
    }

    #[test]
    fn touch_1_is_pinned() {
        assert_eq!(TOUCH.name(), "touch@1");
        assert_eq!(
            TOUCH.rule,
            "drop a said row when none of its files was touched earlier in the session, unless it is an issue or a handoff"
        );
        let row = |kind: &str, key: Option<&str>| crate::Row {
            kind: kind.into(),
            key: key.map(Into::into),
            ..crate::Row::default()
        };
        assert!(touch_drops(&row("decision", None), 0));
        assert!(!touch_drops(&row("decision", None), 1));
        assert!(!touch_drops(&row("issue", None), 0));
        assert!(!touch_drops(&row("note", Some("plan:x:handoff")), 0));
        assert!(touch_drops(&row("note", Some("plan:x:scope")), 0));
    }
}
