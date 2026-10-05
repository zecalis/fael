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

/// Shadow candidate (chunk 4): `touch@1` held back by what the row earned
/// before. A said row is engaged when the agent cited it, pulled it itself or
/// acted on it; its history is the first of (row, file, trigger), (row, file),
/// (row), (class) with enough earlier sessions. The decay window (how many
/// earlier sessions count) is a parameter `tune` sweeps: releasing the policy
/// pins it as a new version.
pub const TOUCH_YIELD: PolicyDef = PolicyDef {
    id: "touch-yield",
    version: 1,
    rule: "drop a said row when touch@1 would, and the engaged share of the first of (row,file,trigger), (row,file), (row), (class) with at least 5 earlier sessions is under 10%; keep it when no level has 5",
};

/// Earlier sessions a history level needs before it speaks.
pub const YIELD_MIN_N: usize = 5;
/// The engaged share (percent) under which a history level calls a row low-yield.
pub const YIELD_FLOOR_PCT: usize = 10;

/// Would `touch-yield@1` drop `r`? `hist` = (engaged, sessions) of the first
/// history level with enough sessions; `None` = no level had enough, and a row
/// with no evidence is kept, never guessed at.
pub fn touch_yield_drops(r: &crate::Row, touch: usize, hist: Option<(usize, usize)>) -> bool {
    touch_drops(r, touch)
        && hist.is_some_and(|(x, n)| n >= YIELD_MIN_N && x * 100 < YIELD_FLOOR_PCT * n)
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

    #[test]
    fn touch_yield_1_is_pinned() {
        assert_eq!(TOUCH_YIELD.name(), "touch-yield@1");
        assert_eq!(
            TOUCH_YIELD.rule,
            "drop a said row when touch@1 would, and the engaged share of the first of (row,file,trigger), (row,file), (row), (class) with at least 5 earlier sessions is under 10%; keep it when no level has 5"
        );
        assert_eq!((YIELD_MIN_N, YIELD_FLOOR_PCT), (5, 10));
        let d = crate::Row {
            kind: "decision".into(),
            ..crate::Row::default()
        };
        assert!(touch_yield_drops(&d, 0, Some((0, 5))));
        assert!(!touch_yield_drops(&d, 0, Some((1, 10)))); // 10% is not under 10%
        assert!(touch_yield_drops(&d, 0, Some((0, 100))));
        assert!(!touch_yield_drops(&d, 0, Some((0, 4))), "too few sessions");
        assert!(!touch_yield_drops(&d, 0, None), "no evidence keeps the row");
        assert!(!touch_yield_drops(&d, 1, Some((0, 50))), "touched stays");
    }
}
