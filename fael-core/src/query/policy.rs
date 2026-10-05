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
}
