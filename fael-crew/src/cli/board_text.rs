//! The terminal view of the board.

use super::*;

/// The same lists for a person at a terminal: one line per chunk. Blocked folds into per-plan
/// counts unless a `--plan` cut is active — 78 lines made 14k of a 22k board.
/// Uids of gated drafts (draft chunks outside inbox, issue 01M59BNV): waiting on
/// a brief or a gate, listed only under a `--plan` cut — never in the queue.
fn gated<'a>(b: &'a Board<'a>, plan_cut: bool) -> Vec<(&'a str, &'a str)> {
    if !plan_cut {
        return Vec::new();
    }
    b.projects
        .iter()
        .flat_map(|p| &p.chunks)
        .filter(|c| c.state == "draft" && c.plan != "inbox")
        .map(|c| (c.uid.as_str(), "gated"))
        .collect()
}

pub(super) fn text(b: &Board, plan_cut: bool) -> String {
    let all: HashMap<&str, &BoardChunk> = b
        .projects
        .iter()
        .flat_map(|p| &p.chunks)
        .map(|c| (c.uid.as_str(), c))
        .collect();
    let mut out = String::new();
    for p in b.projects.iter().filter(|p| p.error.is_some()) {
        out.push_str(&format!(
            "unreadable {}: {}\n",
            p.root,
            p.error.as_deref().unwrap_or("")
        ));
    }
    // Done elsewhere: an open chunk whose linked issue is closed, or whose
    // brief cites a closed row — the board already knew (`stale`), it just
    // never said it. One line per chunk, in every view.
    let stale = stale_rows(b);
    // Gated drafts show only under a `--plan` cut, never queued (see `gated`).
    for (head, rows) in [
        (
            "needs you",
            b.needs_you
                .iter()
                // `merged` = its branch landed while the run is live: say why it is also running
                .map(|n| match n.kind {
                    "merged" => (n.uid.as_str(), "merged, still running"),
                    k => (n.uid.as_str(), k),
                })
                .collect::<Vec<_>>(),
        ),
        ("stale", stale),
        (
            "queue",
            b.queue.iter().map(|u| (u.as_str(), "ready")).collect(),
        ),
        (
            "running",
            b.running.iter().map(|u| (u.as_str(), "running")).collect(),
        ),
        (
            "blocked",
            b.blocked
                .iter()
                .filter(|_| plan_cut)
                .map(|u| (u.as_str(), "blocked"))
                .collect(),
        ),
        ("gated", gated(b, plan_cut)),
    ] {
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("{head}\n"));
        for (uid, tag) in rows {
            out.push_str(&chunk_line(&all, uid, tag));
        }
    }
    if !plan_cut && !b.blocked.is_empty() {
        let mut n: Vec<(&str, usize)> = Vec::new();
        for u in &b.blocked {
            let plan = all[u.as_str()].plan.as_str();
            match n.iter_mut().find(|(p, _)| *p == plan) {
                Some(e) => e.1 += 1,
                None => n.push((plan, 1)),
            }
        }
        n.sort_by_key(|&(_, k)| Reverse(k)); // stable: first-seen plan breaks ties
        let per: Vec<String> = n.iter().map(|(p, k)| format!("{p} {k}")).collect();
        out.push_str(&format!(
            "blocked  {} — {} — `fael board --plan <p>` lists them\n",
            b.blocked.len(),
            per.join(" · ")
        ));
    }
    if out.is_empty() {
        out.push_str(&empty_msg(b, plan_cut));
    }
    out.push_str(&models_card(b));
    out.push_str(&repos_line(b));
    out
}

/// Cost-per-feature read (run-telemetry t5): totals with unknown-% beside
/// every cost number, per-model sums, and the 30-run verdict window.
/// Old NULL rows stay NULL — the unknown-% is the point, not a gap to fill.
/// A cost unknown-% over half hides the cost numbers (issue 01M6B4CX): a sum
/// over ~nothing reads as a total and gets planned against. The unknown-%
/// stays — it is the signal — and JSON keeps every number (machine contract).
const COST_HIDE_PCT: f64 = 50.0;

fn models_card(b: &Board) -> String {
    let m = &b.models;
    if m.total == 0 {
        return String::new();
    }
    let hide_cost = m.unknown_cost_pct > COST_HIDE_PCT;
    let mut out = String::from("models\n");
    if hide_cost {
        out.push_str(&format!(
            "  runs {} · with model {} (unknown {:.1}%) · cost hidden (unknown {:.1}% > {:.0}%)\n",
            m.total, m.with_client_model, m.unknown_model_pct, m.unknown_cost_pct, COST_HIDE_PCT
        ));
    } else {
        out.push_str(&format!(
            "  runs {} · with model {} (unknown {:.1}%) · with cost {} (unknown {:.1}%)\n",
            m.total, m.with_client_model, m.unknown_model_pct, m.with_cost, m.unknown_cost_pct
        ));
    }
    let mut by: Vec<(&String, &fael_crew::ModelRow)> = m.by_model.iter().collect();
    by.sort_by_key(|(_, r)| std::cmp::Reverse(r.runs));
    let shown = 5;
    let parts: Vec<String> = by
        .iter()
        .take(shown)
        .map(|(k, r)| {
            if hide_cost {
                format!("{k} {} runs (cost hidden)", r.runs)
            } else {
                format!(
                    "{k} {} runs ${:.6} (unknown {:.1}%)",
                    r.runs, r.cost, r.unknown_cost_pct
                )
            }
        })
        .collect();
    if !parts.is_empty() {
        let more = by.len().saturating_sub(parts.len());
        let tail = if more > 0 {
            format!(" (+{more} more)")
        } else {
            String::new()
        };
        out.push_str(&format!("  by model: {}{tail}\n", parts.join(" · ")));
    }
    let w = &m.window;
    let verdict = match w.pass {
        Some(true) => "pass",
        Some(false) => "fail",
        None => "pending",
    };
    let mp = w.model_pct.map_or("—".into(), |v| format!("{v:.1}%"));
    let cp = if hide_cost {
        "—".to_string()
    } else {
        w.cost_pct.map_or("—".into(), |v| format!("{v:.1}%"))
    };
    out.push_str(&format!(
        "  window {}/{}: {verdict} (model {mp}, cost {cp})\n",
        w.n, w.need
    ));
    out
}

/// One terminal row: the plan, the cut title, and why this list holds it.
fn chunk_line(all: &HashMap<&str, &BoardChunk>, uid: &str, tag: &str) -> String {
    let c = all[uid];
    let label = c
        .label
        .as_deref()
        .map(|l| format!(" {l}"))
        .unwrap_or_default();
    let extra = match tag {
        "waiting" => c.wait.as_ref().and_then(|w| w.text.clone()),
        // a stale chunk names why: the first warning, without its prefix
        "stale" => c
            .stale
            .first()
            .map(|w| w.strip_prefix("warning: ").unwrap_or(w).to_string()),
        // a lead's overlap check cannot place it, so its set skips it (01M4WZ4M)
        "ready" if c.scope.is_empty() => Some("no scope".into()),
        "blocked" => Some(
            c.blocked_by
                .iter()
                .map(|x| x.what.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
        .map(|w| format!("after {w}")),
        _ => None,
    }
    .map(|x| format!(" — {x}"))
    .unwrap_or_default();
    // ponytail: a terminal line, cut at 80 chars; the app reads --json
    let title: String = match c.title.char_indices().nth(80) {
        Some((i, _)) => format!("{}…", &c.title[..i]),
        None => c.title.clone(),
    };
    format!("  {tag:<8} {uid}  {}{label}  {title}{extra}\n", c.plan)
}

/// Done elsewhere: an open chunk whose linked issue is closed, or whose
/// brief cites a closed row — the board already knew (`stale`), it just
/// never said it. One line per chunk, in every view.
fn stale_rows<'a>(b: &'a Board<'a>) -> Vec<(&'a str, &'a str)> {
    b.projects
        .iter()
        .flat_map(|p| &p.chunks)
        .filter(|c| !c.stale.is_empty())
        .map(|c| (c.uid.as_str(), "stale"))
        .collect()
}

/// What an empty board prints: a `--plan` cut on a parked or done plan names
/// the hidden work; a cut on an active plan keeps the old nothing-here line;
/// no cut with repos but no chunks names this repo's empty state and its next
/// command before the other repos' rows.
fn empty_msg(b: &Board, plan_cut: bool) -> String {
    if plan_cut && let Some(msg) = parked_done_msg(b) {
        return msg;
    }
    if plan_cut || b.projects.is_empty() {
        return "nothing on the board — `fael chunk add` or `fael plan new` in a repo\n".into();
    }
    "this repo has no chunks — start one: `fael plan new <name> \"<title>\"` then `fael chunk add \"<title>\" --plan <plan>`\n".into()
}

/// A `--plan` cut on a parked or done plan lists no chunks (the board only
/// lists active plans' chunks): say the plan is parked/done, how many open
/// chunks it hides, and the retype that lists them — the same wording
/// `chunk show` uses for a hidden uid (issue 01M5KVYE).
fn parked_done_msg(b: &Board) -> Option<String> {
    let mut out = String::new();
    for p in b.projects.iter().flat_map(|p| &p.plans) {
        if !matches!(p.state.as_str(), "parked" | "done") {
            continue;
        }
        let key = if p.app.is_empty() {
            p.name.clone()
        } else {
            format!("{}/{}", p.app, p.name)
        };
        let hidden: usize = p
            .counts
            .iter()
            .filter(|(s, _)| !matches!(s.as_str(), "done" | "dropped" | "replaced"))
            .map(|(_, n)| n)
            .sum();
        let chunks = if hidden == 1 {
            "1 open chunk".to_string()
        } else {
            format!("{hidden} open chunks")
        };
        if p.state == "parked" {
            out.push_str(&format!(
                "plan {key} is parked — {chunks} hidden; `fael plan unpark {key}` first, or `fael plan export {key}` lists its uids\n"
            ));
        } else {
            out.push_str(&format!(
                "plan {key} is done — {chunks} hidden; `fael plan undone {key}` first, or `fael plan export {key}` lists its uids\n"
            ));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The roots the board merged (issue 01M5H0VZ): human mode never showed
/// where it looked, so an empty sweep elsewhere read as nothing to do.
fn repos_line(b: &Board) -> String {
    if b.projects.is_empty() {
        return String::new();
    }
    let mut roots: Vec<&str> = b.projects.iter().map(|p| p.root.as_str()).collect();
    roots.sort_unstable();
    roots.dedup();
    format!("repos: {}\n", roots.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn models(unknown_cost_pct: f64, with_cost: usize, total: usize) -> fael_crew::Models {
        let mut by_model = BTreeMap::new();
        by_model.insert(
            "opencode/opencode-go/m".to_string(),
            fael_crew::ModelRow {
                runs: total,
                cost: 1.5,
                unknown_cost_pct,
                missing: 0,
            },
        );
        fael_crew::Models {
            total,
            with_client_model: total,
            with_cost,
            unknown_model_pct: 0.0,
            unknown_cost_pct,
            by_model,
            window: fael_crew::CostWindow {
                n: 0,
                need: 30,
                model_pct: None,
                cost_pct: Some(40.0),
                pass: None,
            },
        }
    }

    fn card(unknown_cost_pct: f64, with_cost: usize, total: usize) -> String {
        models_card(&Board {
            v: 1,
            at: String::new(),
            needs_you: Vec::new(),
            queue: Vec::new(),
            running: Vec::new(),
            blocked: Vec::new(),
            queue_exclusive: Vec::new(),
            queue_joint: Vec::new(),
            blocked_on_overlap: Vec::new(),
            models: models(unknown_cost_pct, with_cost, total),
            projects: Vec::new(),
        })
    }

    #[test]
    fn cost_hidden_past_half_but_unknown_stays() {
        let t = card(97.5, 1, 40);
        assert!(t.contains("cost hidden (unknown 97.5% > 50%)"), "{t}");
        assert!(!t.contains("with cost"), "{t}");
        assert!(!t.contains('$'), "{t}");
        assert!(t.contains("(model —, cost —)"), "{t}");
    }

    #[test]
    fn cost_shown_at_or_below_half() {
        for u in [0.0, 50.0] {
            let t = card(u, 2, 2);
            assert!(t.contains("with cost 2"), "{u}: {t}");
            assert!(t.contains("$1.500000"), "{u}: {t}");
            assert!(t.contains("cost 40.0%"), "{u}: {t}");
        }
    }
}
