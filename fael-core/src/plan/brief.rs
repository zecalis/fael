//! What `fael chunk start` prints — the agent's first message (SPEC §8). The first line
//! names R; the plan md's Goal, Scope, Done criteria and Constraints are read live by
//! heading (SPEC §9), and a ref that no longer exists is listed as missing.

use super::start::Started;
use std::fmt::Write;

/// How a chunk's state moves, said once at the end of every brief and kickoff.
pub fn rules(uid: &str) -> String {
    format!(
        "## chunk rules\n\
         - state moves only through fael, never by editing a plan file:\n  \
         finished → `fael chunk done {uid} \"<handoff>\" --pr N` (or `--out <path>` for a file, image or video): review, the owner merges or accepts\n  \
         need the owner → `fael chunk wait {uid} --on owner \"<question>\"` · need data → `--on data \"<what>\" --until YYYY-MM-DD`\n  \
         needs another chunk first → `fael chunk after {uid} <other-uid> \"<why>\"`\n\
         - one chunk = one branch = one PR; never merge, the owner does\n\
         - work left over → `fael chunk add \"<title>\" --plan <plan> --brief \"…\"` · a fact the next session needs → `fael add note … --files <f>`\n"
    )
}

/// The md sections a brief carries: `## Goal`, `## 2. Scope`, `## Done criteria`,
/// `## Constraints` (any numbering), each up to the next `## `.
pub fn sections(md: &str) -> String {
    let mut out = String::new();
    let mut keep = false;
    for l in md.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            let h = h.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ');
            keep = ["Goal", "Scope", "Done", "Constraints"]
                .iter()
                .any(|w| h.starts_with(w));
        }
        if keep {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

impl Started {
    /// `md(source)` = the plan file's text, `exists(path)` = whether a ref is still there.
    pub fn text(
        &self,
        md: &dyn Fn(&str) -> Option<String>,
        exists: &dyn Fn(&str) -> bool,
    ) -> String {
        let mut o = String::new();
        let n = self.chunks.len();
        let what = if n == 1 {
            "1 chunk".to_string()
        } else {
            format!("{n} paired chunks")
        };
        let _ = writeln!(o, "fael run {} · {what}", self.run);
        for c in &self.chunks {
            let label = c
                .label
                .as_deref()
                .map(|l| format!("{l} — "))
                .unwrap_or_default();
            let _ = writeln!(
                o,
                "\n# {label}{} ({})\nplan {} — {}",
                c.title, c.uid, c.plan, c.plan_title
            );
            let fields: Vec<String> = [("size", &c.size), ("model", &c.model_hint)]
                .iter()
                .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k} {v}")))
                .chain(
                    c.scope
                        .as_ref()
                        .map(|s| format!("scope {}", s.replace('\n', ", "))),
                )
                .collect();
            if !fields.is_empty() {
                let _ = writeln!(o, "{}", fields.join(" · "));
            }
            if c.brief != c.title {
                let _ = writeln!(o, "\n{}", c.brief.trim_end());
            }
            if !c.said.is_empty() {
                o.push_str("\nowner said:\n");
                for s in &c.said {
                    let _ = writeln!(o, "> {}", s.replace('\n', "\n> "));
                }
            }
        }
        // plan context once per plan, after the chunks
        let mut seen = Vec::new();
        for c in &self.chunks {
            if seen.contains(&&c.plan) {
                continue;
            }
            seen.push(&c.plan);
            if let Some(s) = (!c.source.is_empty()).then(|| md(&c.source)).flatten() {
                let s = sections(&s);
                if !s.is_empty() {
                    let _ = write!(o, "\n<!-- {} -->\n{s}", c.source);
                }
            }
            if !c.refs.is_empty() {
                let refs: Vec<String> = c
                    .refs
                    .iter()
                    .map(|r| {
                        let path = r.split('#').next().unwrap_or(r);
                        if exists(path) {
                            r.clone()
                        } else {
                            format!("{r} (missing)")
                        }
                    })
                    .collect();
                let _ = writeln!(o, "\nrefs: {}", refs.join(" · "));
            }
        }
        o.push('\n');
        o.push_str(&rules(self.chunks.first().map_or("<uid>", |c| &c.uid)));
        o
    }
}

#[cfg(test)]
mod tests {
    use super::super::start::{Brief, Started};
    use super::*;

    const DOC: &str = "# PLAN-x\n\n## TL;DR\n- [ ] 1 — one\n\n## 1. Goal\nship it\n\n\
                       ## 2. Scope\nonly cli\n\n## 3. Done criteria\ntests pass\n\n\
                       ## 4. Constraints\n400 lines\n\n## 5. Risks\nnone said\n";

    fn chunk(uid: &str) -> Brief {
        Brief {
            uid: uid.into(),
            label: Some("1".into()),
            title: format!("title {uid}"),
            brief: format!("brief {uid}"),
            size: None,
            model_hint: None,
            scope: None,
            plan: "x".into(),
            plan_title: "PLAN-x".into(),
            source: ".fapony/plan/PLAN-x.md".into(),
            refs: vec!["SPEC-x.md#block".into(), "gone.rs".into()],
            said: vec![],
        }
    }

    #[test]
    fn the_brief_reads_the_plan_md_live_and_marks_missing_refs() {
        let s = sections(DOC);
        for keep in [
            "## 1. Goal\nship it",
            "## 2. Scope",
            "## 3. Done criteria",
            "## 4. Constraints\n400 lines",
        ] {
            assert!(s.contains(keep), "{keep} in {s}");
        }
        assert!(!s.contains("TL;DR") && !s.contains("Risks"), "{s}");
        let st = Started {
            run: "R".into(),
            chunks: vec![chunk("A"), chunk("B")],
        };
        let md = |src: &str| (src == ".fapony/plan/PLAN-x.md").then(|| DOC.to_string());
        let t = st.text(&md, &|p| p == "SPEC-x.md");
        assert!(t.starts_with("fael run R · 2 paired chunks\n"), "{t}");
        assert!(t.contains("brief A") && t.contains("brief B"), "{t}");
        assert_eq!(
            t.matches("## 1. Goal").count(),
            1,
            "plan context once per plan: {t}"
        );
        assert!(
            t.contains("refs: SPEC-x.md#block · gone.rs (missing)"),
            "{t}"
        );
        assert!(t.contains("fael chunk done A"), "{t}");
        // no plan file (inbox, or a moved md): the brief still prints
        let t = st.text(&|_| None, &|_| true);
        assert!(
            !t.contains("Goal") && t.contains("refs: SPEC-x.md#block · gone.rs\n"),
            "{t}"
        );
    }
}
