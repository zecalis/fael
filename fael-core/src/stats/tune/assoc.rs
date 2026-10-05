//! Observed association between a row's features and what followed it.
//! Descriptive: a rate per feature value, and phi for a yes/no feature. Not a
//! cause, and not a score — nothing here ranks a policy.

use super::Ob;
use super::rate::{Rate, phi};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Group {
    pub value: String,
    pub n: usize,
    pub cited: Rate,
    pub pulled: Rate,
    pub acted: Rate,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Assoc {
    pub feature: &'static str,
    pub groups: Vec<Group>,
    /// Phi of the feature's yes/no split against cited, pulled, acted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phi: Option<[Option<f64>; 3]>,
}

fn outcomes(o: &Ob) -> [bool; 3] {
    [o.o.cited, o.o.agent_pull, o.o.acted]
}

fn groups(obs: &[&Ob], of: impl Fn(&Ob) -> Option<String>) -> Vec<Group> {
    let mut m: BTreeMap<String, (usize, [usize; 3])> = BTreeMap::new();
    for o in obs {
        let Some(v) = of(o) else { continue };
        let e = m.entry(v).or_default();
        e.0 += 1;
        for (c, hit) in e.1.iter_mut().zip(outcomes(o)) {
            *c += hit as usize;
        }
    }
    m.into_iter()
        .map(|(value, (n, c))| Group {
            value,
            n,
            cited: Rate::new(c[0], n),
            pulled: Rate::new(c[1], n),
            acted: Rate::new(c[2], n),
        })
        .collect()
}

/// Phi of a yes/no feature against each outcome.
fn phis(obs: &[&Ob], yes: impl Fn(&Ob) -> Option<bool>) -> [Option<f64>; 3] {
    let mut c = [[0usize; 4]; 3];
    for o in obs {
        let Some(f) = yes(o) else { continue };
        for (k, hit) in outcomes(o).into_iter().enumerate() {
            // a = both, b = feature only, c = outcome only, d = neither
            c[k][match (f, hit) {
                (true, true) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (false, false) => 3,
            }] += 1;
        }
    }
    c.map(|[a, b, c, d]| phi(a, b, c, d))
}

pub(super) fn assoc(obs: &[&Ob]) -> Vec<Assoc> {
    let age = |o: &Ob| {
        let d = o.feat["age_d"].as_u64()?;
        Some(["0-6", "7-29", "30+"][(d >= 7) as usize + (d >= 30) as usize].to_string())
    };
    let yes_touch = |o: &Ob| o.touch.map(|t| t > 0);
    let yes_hub = |o: &Ob| o.feat["hub"].as_bool();
    vec![
        Assoc {
            feature: "touch",
            groups: groups(obs, |o| {
                o.touch.map(|t| ["0", "1", "2+"][t.min(2)].to_string())
            }),
            phi: Some(phis(obs, yes_touch)),
        },
        Assoc {
            feature: "hub",
            groups: groups(obs, |o| o.feat["hub"].as_bool().map(|b| b.to_string())),
            phi: Some(phis(obs, yes_hub)),
        },
        Assoc {
            feature: "trigger",
            groups: groups(obs, |o| {
                Some(o.trigger.to_string()).filter(|t| !t.is_empty())
            }),
            phi: None,
        },
        Assoc {
            feature: "kind",
            groups: groups(obs, |o| o.feat["kind"].as_str().map(str::to_string)),
            phi: None,
        },
        Assoc {
            feature: "age_d",
            groups: groups(obs, age),
            phi: None,
        },
    ]
}
