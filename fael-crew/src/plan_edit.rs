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
        self.edit_section(plan, key, now, |body, at| match body.matches(old).count() {
            1 => Ok(body.replacen(old, new, 1)),
            0 => Err(format!(
                "rejected: {old:?} is not in {at} (0 matches) — try copying it exactly: `fael plan export {} --section {key}`",
                plan.name
            )),
            n => Err(format!(
                "rejected: {old:?} occurs {n} times in {at} — try a longer passage until it matches once"
            )),
        })
    }

    /// Add `text` as a new paragraph at the end of section `key` of `plan`, through the same
    /// bill as `plan_edit` (issue 01M6BKHZM).
    pub fn plan_append(
        &mut self,
        plan: &PlanRow,
        key: &str,
        text: &str,
        now: &str,
    ) -> Result<(Vec<String>, i64), String> {
        if text.trim().is_empty() {
            return Err(
                "rejected: --append needs the text to add; try --append \"<paragraph>\"".into(),
            );
        }
        self.edit_section(plan, key, now, |body, _| {
            Ok(match body.trim_end() {
                "" => text.trim_end().to_string(),
                b => format!("{b}\n\n{}", text.trim_end()),
            })
        })
    }

    /// The section `key`'s new body from `f(body, "section k of plan")`, written as a one-section bill.
    fn edit_section(
        &mut self,
        plan: &PlanRow,
        key: &str,
        now: &str,
        f: impl FnOnce(&str, &str) -> Result<String, String>,
    ) -> Result<(Vec<String>, i64), String> {
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
        let body = f(&sec.body, &format!("section {} of {}", sec.key, plan.name))?;
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
