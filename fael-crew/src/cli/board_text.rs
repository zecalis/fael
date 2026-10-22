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
    let line = |uid: &str, tag: &str| {
        let c = all[uid];
        let label = c
            .label
            .as_deref()
            .map(|l| format!(" {l}"))
            .unwrap_or_default();
        let extra = match tag {
            "waiting" => c.wait.as_ref().and_then(|w| w.text.clone()),
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
    };
    let mut out = String::new();
    for p in b.projects.iter().filter(|p| p.error.is_some()) {
        out.push_str(&format!(
            "unreadable {}: {}\n",
            p.root,
            p.error.as_deref().unwrap_or("")
        ));
    }
    // Gated drafts show only under a `--plan` cut, never queued (see `gated`).
    for (head, rows) in [
        (
            "needs you",
            b.needs_you
                .iter()
                .map(|n| (n.uid.as_str(), n.kind))
                .collect::<Vec<_>>(),
        ),
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
            out.push_str(&line(uid, tag));
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
        out.push_str("nothing on the board — `fael chunk add` or `fael plan new` in a repo\n");
    }
    out.push_str(&repos_line(b));
    out
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
