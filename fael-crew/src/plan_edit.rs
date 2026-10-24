//! `fael plan edit <plan> --section <key> --replace <old> --with <new>`: one passage of a
//! section swapped, through the same bill as `plan apply` (one rev check, one rev bump, one
//! event; decision 01M4V39ZP). Never a guess: `<old>` must occur in the section body once.

use super::bill::Bill;
use super::export::export_json;
use super::section;
use super::store::{PlanRow, Store};
use serde_json::json;

impl Store {
    /// Replace the one occurrence of `old` in section `key` of `plan` with `new`; returns
    /// the bill's lines and the plan's rev now. Rejects 0 or N occurrences, writing nothing.
    pub fn plan_edit(
        &mut self,
        plan: &PlanRow,
        key: &str,
        old: &str,
        new: &str,
        now: &str,
    ) -> Result<(Vec<String>, i64), String> {
        if old.is_empty() {
            return Err(
                "rejected: --replace needs the text to replace, not an empty string; try --replace \"<old>\" --with \"<new>\"".into(),
            );
        }
        // rev first: a writer between the two reads makes the bill's rev check reject as stale
        let rev = export_json(self, plan.id)?["rev"]
            .as_i64()
            .unwrap_or_default();
        let secs = section::list(&self.conn, plan.id)?;
        let Some(sec) = section::resolve(&secs, key) else {
            let keys: Vec<&str> = secs.iter().map(|r| r.key.as_str()).collect();
            return Err(format!(
                "rejected: no section {key} — the plan has: {}; try --section <one of these>",
                keys.join(", ")
            ));
        };
        let at = format!("section {} of {}", sec.key, plan.name);
        match sec.body.matches(old).count() {
            1 => {}
            0 => {
                return Err(format!(
                    "rejected: {old:?} is not in {at} (0 matches) — try copying it exactly: `fael plan export {} --section {}`",
                    plan.name, sec.key
                ));
            }
            n => {
                return Err(format!(
                    "rejected: {old:?} occurs {n} times in {at} — try a longer passage until it matches once"
                ));
            }
        }
        let body = sec.body.replacen(old, new, 1);
        let name = if plan.app.is_empty() {
            plan.name.clone()
        } else {
            format!("{}/{}", plan.app, plan.name)
        };
        let bill = json!({"v": 1, "plan": name, "rev": rev,
            "sections": [{"key": sec.key, "body": body}]});
        let bill: Bill = serde_json::from_value(bill).map_err(|e| e.to_string())?;
        self.bill(bill, now, false)
    }
}

#[cfg(test)]
#[path = "plan_edit_tests.rs"]
mod tests;
