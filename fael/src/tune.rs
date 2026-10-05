//! `fael tune [--json] [--since d]` — SPEC-fael-learn-loop §C. Core replays the
//! candidates (`core::stats::tune`); this prints them. Read-only: it opens
//! `usage.jsonl` and the logs and writes nothing. It never names a winner —
//! a human reads the table and files a decision.

use crate::args::Args;
use crate::core::stats::{Coverage, PolicyResult, Rate, Section, Stratum, Tune};
use crate::{hook, report};

pub(crate) fn tune(a: &Args) -> Result<(), String> {
    a.only("tune", &["json", "since"])?;
    let u = hook::load(report::since(a)?);
    let t = crate::core::stats::tune(&u.parsed, &u.logs, hook::local_tz_offset_min());
    if a.has("json") {
        println!("{}", serde_json::to_string(&t).map_err(|e| e.to_string())?);
    } else {
        print!("{}", text(&t));
    }
    Ok(())
}

/// `1.9%` under ten, `51%` above — the interval's ends are rounded alike.
fn pc(v: f64) -> String {
    if v < 10.0 {
        format!("{v:.1}%")
    } else {
        format!("{v:.0}%")
    }
}

/// `41/2200 1.9% [1.4–2.5%]`; no `n` has no rate.
fn rate(r: &Rate) -> String {
    match (r.pct(), r.lo, r.hi) {
        (Some(p), Some(lo), Some(hi)) => format!(
            "{}/{} {} [{}–{}]",
            r.x,
            r.n,
            pc(p),
            pc(lo * 100.0).trim_end_matches('%'),
            pc(hi * 100.0)
        ),
        _ => "—".to_string(),
    }
}

fn policy(p: &PolicyResult, indent: &str) -> String {
    let r = &p.retained;
    format!(
        "{indent}{} — {} rows would not be said\n\
         {indent}  exposure     {} rows still said · {} pushes left silent\n\
         {indent}  retained     cited {} · pulled {} · acted {}\n\
         {indent}  retrieved    rows {} · sessions {}\n\
         {indent}  missed_push  {}\n",
        p.policy,
        p.dropped,
        rate(&p.exposure),
        rate(&p.pushes_silenced),
        rate(&r.cited),
        rate(&r.pulled),
        rate(&r.acted),
        rate(&p.retrieved_after_cut.rows),
        rate(&p.retrieved_after_cut.sessions),
        rate(&p.missed_push),
    )
}

fn coverage(c: &Coverage) -> String {
    use crate::core::stats::{MAX_DAY_SHARE_PCT, MAX_SESSION_SHARE_PCT, MIN_DAYS};
    format!(
        "coverage: {} days · busiest day {} · busiest session {} — {} the provisional thresholds (≥{MIN_DAYS} days, ≤{MAX_DAY_SHARE_PCT}%, ≤{MAX_SESSION_SHARE_PCT}%)\n",
        c.distinct_days,
        pc(c.max_day_share_pct),
        pc(c.max_session_share_pct),
        if c.passes { "within" } else { "outside" },
    )
}

fn section(s: &Section) -> String {
    let z = &s.sizes;
    let mut out = format!(
        "arm sizes: sessions {} · search pushes {} · rows said {} (replayable {}) · recorded would_drop {}\n{}",
        z.sessions,
        z.search_pushes,
        z.rows_said,
        z.evaluable_rows,
        z.recorded_would_drop,
        coverage(&s.coverage),
    );
    if z.evaluable_rows == 0 {
        out.push_str(
            "no said search row has a recorded working set yet (feat.touch): nothing to replay\n",
        );
        return out;
    }
    for p in &s.policies {
        out.push_str(&policy(p, ""));
    }
    out.push_str("association (observed, not cause) — outcome per feature value\n");
    for a in &s.association {
        for g in &a.groups {
            out.push_str(&format!(
                "  {:8} {:5} n={} · cited {}/{} · pulled {}/{} · acted {}/{}\n",
                a.feature, g.value, g.n, g.cited.x, g.n, g.pulled.x, g.n, g.acted.x, g.n
            ));
        }
        if let Some(p) = a.phi {
            let f = |v: Option<f64>| v.map_or("—".into(), |v| format!("{v:+.2}"));
            out.push_str(&format!(
                "  {:8} phi (yes/no split): cited {} · pulled {} · acted {}\n",
                a.feature,
                f(p[0]),
                f(p[1]),
                f(p[2])
            ));
        }
    }
    let u = &s.fallback_used;
    out.push_str(&format!(
        "history used by touch-yield@1 (decay none, rows touch@1 would cut; a level speaks at n ≥ 5): (row,file,trigger) {} · (row,file) {} · (row) {} · (class) {} · no level {}\n",
        u[0], u[1], u[2], u[3], u[4]
    ));
    out.push_str("decay sweep (touch-yield@1, earlier sessions counted per key):\n");
    for d in &s.decay_sweep {
        out.push_str(&format!(
            "  {:5} cuts {} · exposure {} · missed_push {}\n",
            d.decay.map_or("none".into(), |n| n.to_string()),
            d.result.dropped,
            rate(&d.result.exposure),
            rate(&d.result.missed_push),
        ));
    }
    out
}

pub(crate) fn text(t: &Tune) -> String {
    let span = t
        .days
        .as_ref()
        .map_or(String::new(), |(a, b)| format!(" {a}..{b}"));
    let mut out = format!(
        "fael tune · search pushes{span} · outcomes_v {} · decay: none\n\
         a replay of candidate rules over what fael already said — it chooses nothing and writes nothing\n",
        t.outcomes_v
    );
    out.push_str(&section(&t.all));
    if t.strata.len() > 1 {
        out.push_str("by stratum (repo x client):\n");
        t.strata
            .iter()
            .take(10)
            .for_each(|s| out.push_str(&stratum(s)));
    }
    if t.strata.len() > 10 {
        out.push_str(&format!("  … +{} more (--json)\n", t.strata.len() - 10));
    }
    out
}

fn stratum(s: &Stratum) -> String {
    let z = &s.section.sizes;
    let mut out = format!(
        "  {} / {} — sessions {} · search pushes {} · {}\n",
        s.repo,
        s.client,
        z.sessions,
        z.search_pushes,
        if s.eligible {
            "eligible"
        } else {
            "insufficient · report only"
        }
    );
    for p in s
        .section
        .policies
        .iter()
        .skip(1)
        .filter(|_| z.evaluable_rows > 0)
    {
        out.push_str(&format!(
            "    {:14} exposure {} · missed_push {}\n",
            p.policy,
            rate(&p.exposure),
            rate(&p.missed_push)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::stats::Rate;

    fn tune_of(strata: usize) -> Tune {
        Tune {
            outcomes_v: 1,
            days: None,
            all: Section::default(),
            strata: (0..strata)
                .map(|i| Stratum {
                    repo: format!("/r{i}"),
                    client: "claude".into(),
                    eligible: false,
                    section: Section::default(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_rate_shows_x_of_n_with_its_interval_and_no_n_is_a_dash() {
        assert_eq!(rate(&Rate::new(0, 0)), "—");
        assert_eq!(rate(&Rate::new(41, 2200)), "41/2200 1.9% [1.4–2.5%]");
        assert_eq!(rate(&Rate::new(0, 20)), "0/20 0.0% [0.0–16%]");
        assert_eq!(rate(&Rate::new(61, 120)), "61/120 51% [42–60%]");
    }

    #[test]
    fn strata_print_ten_then_count_the_rest_and_one_stratum_prints_no_header() {
        let out = text(&tune_of(12));
        assert!(out.contains("by stratum"), "{out}");
        assert!(out.contains("/r9") && !out.contains("/r10"), "{out}");
        assert!(out.contains("… +2 more (--json)"), "{out}");
        let one = text(&tune_of(1));
        assert!(!one.contains("by stratum") && !one.contains("/r0"), "{one}");
    }

    #[test]
    fn nothing_replayable_says_so_and_prints_no_policy_table() {
        let out = text(&tune_of(0));
        assert!(out.contains("nothing to replay"), "{out}");
        assert!(!out.contains("baseline@1"), "{out}");
    }
}
