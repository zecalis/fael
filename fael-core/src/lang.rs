//! Language packs for the Stop-hook markers and the row-language warning
//! (PLAN-fael-languages chunk 1): every phrase lives behind `.fael/config.toml`
//! `[lang]`, so this file names no language of its own — adding one is a new
//! `Lang` below plus one arm in `by_name`. Core never spawns here: matching
//! and detection are pure text.

use crate::Config;
use std::ops::RangeInclusive;

/// One language pack: the Stop-hook phrases that fire in this language, the
/// words that cancel them, and the alphabet that counts as "this language"
/// for the row warning.
#[derive(Debug, Clone, Copy)]
pub struct Lang {
    pub name: &'static str,
    /// Strong phrases: a confirmed-bug announcement (blocks without an issue row).
    pub bug: &'static [&'static str],
    /// Weak phrases: a risk/inconsistency mention (one-line note, never a block).
    pub risk: &'static [&'static str],
    /// Fix phrases (PLAN-fael-experience-loop chunk 5a): the agent says it
    /// fixed a bug — the moment its cause → fix is worth filing.
    pub fixed: &'static [&'static str],
    /// Strong-cancel window words.
    pub negations: &'static [&'static str],
    /// Weak-cancel window words.
    pub risk_negations: &'static [&'static str],
    /// Weak-cancel sentence words: a risk inside a conditional is a
    /// hypothetical, not a claim ("if both run, the numbers mismatch").
    pub conditionals: &'static [&'static str],
    /// Accepted alphabet ranges.
    pub script: &'static [RangeInclusive<char>],
}

static EN: Lang = Lang {
    name: "english",
    bug: &[
        "found a bug",
        "found the bug",
        "found bug",
        "found a real bug",
        "found the real bug",
        "this is a bug",
        "that is a bug",
        "it is a bug",
        "it's a bug",
        "bug confirmed",
        "confirmed bug",
        "confirmed a bug",
    ],
    risk: &[
        "inconsistent",
        "inconsistency",
        "mismatch",
        "doesn't match",
        "does not match",
        "out of sync",
        "might break",
        "could break",
        "will break",
        "likely to break",
    ],
    fixed: &[
        "fixed the bug",
        "fixed a bug",
        "fixed this bug",
        "fixed the regression",
        "root cause was",
        "root cause is",
    ],
    negations: &["not", "no", "if"],
    risk_negations: &["not", "no"],
    conditionals: &["if", "unless"],
    script: &['A'..='Z', 'a'..='z', 'À'..='ſ', 'ƀ'..='ɏ', 'Ḁ'..='ỿ'],
};

static TH: Lang = Lang {
    name: "thai",
    bug: &["เจอบั๊ก", "พบว่าเป็นบั๊ก", "เจอว่าเป็นบั๊ก", "บั๊กที่เจอ", "บั๊กที่พบ"],
    risk: &[
        "ไม่ตรงกัน",
        "ไม่สอดคล้อง",
        "ขัดแย้งกัน",
        "อาจพัง",
        "น่าจะพัง",
        "อาจมีปัญหา",
        "น่าจะมีปัญหา",
        "มีความเสี่ยง",
    ],
    fixed: &[
        "แก้บั๊กแล้ว",
        "แก้ bug แล้ว",
        "แก้บั๊กเรียบร้อย",
        "สาเหตุของบั๊ก",
        "ต้นเหตุของบั๊ก",
    ],
    negations: &["ไม่", "จะ", "ถ้า", "อาจ"],
    risk_negations: &["ไม่"],
    conditionals: &["ถ้า", "หาก", "สมมติ"],
    // one range is the whole Thai block — the slice shape stays so EN/TH match
    #[allow(clippy::single_range_in_vec_init)]
    script: &['\u{0E00}'..='\u{0E7F}'],
};

/// The pack behind a `[lang]` name (`"english"`, `"thai"`); `None` is an
/// unknown name, which `Config::from_toml` rejects — silently matching
/// nothing would leave the hook blind, worse than an error.
pub fn by_name(name: &str) -> Option<&'static Lang> {
    match name {
        "english" => Some(&EN),
        "thai" => Some(&TH),
        _ => None,
    }
}

/// A matched marker — the exact shape `Hit` the hook adapter reads.
pub struct Hit {
    pub marker: String,
    pub strong: bool,
}

/// The matched marker, or `None`. Code fences, `inline code` and `>` quotes
/// are dropped first — a phrase describing code is not a problem report.
/// Every match is checked against a negation window so "ไม่พบบั๊กใหม่
/// แต่เจอบั๊กที่ X" still fires on the second. The search runs on the
/// lowercased text throughout, so byte indices always belong to the string
/// they slice.
///
/// Negations pool across packs (plus `negations_extra`): with the default
/// `marker = ["english", "thai"]` the sets are exactly the old hardcoded
/// ones, so behaviour is byte-identical. An empty `packs` switches the bug
/// rule off entirely — a repo whose agents use no pack language.
pub fn marker_hit(text: &str, packs: &[&Lang], negations_extra: &[&str]) -> Option<Hit> {
    if packs.is_empty() {
        return None;
    }
    let lower = strip_quoted(text).to_lowercase();
    let mut negs: Vec<&str> = packs
        .iter()
        .flat_map(|p| p.negations.iter().copied())
        .collect();
    negs.extend(negations_extra.iter().copied());
    let mut risk_negs: Vec<&str> = packs
        .iter()
        .flat_map(|p| p.risk_negations.iter().copied())
        .collect();
    risk_negs.extend(negations_extra.iter().copied());
    // phrase lists, pack order
    for pack in packs {
        for p in pack.bug {
            if let Some(i) = lower.find(*p)
                && !negated(&lower, i, &negs)
            {
                return Some(Hit {
                    marker: lower[i..i + p.len()].to_string(),
                    strong: true,
                });
            }
        }
    }
    let conds: Vec<&str> = packs
        .iter()
        .flat_map(|p| p.conditionals.iter().copied())
        .collect();
    for pack in packs {
        for p in pack.risk {
            if let Some(i) = lower.find(*p)
                && !negated(&lower, i, &risk_negs)
                && !conditional(&lower, i, &conds)
            {
                return Some(Hit {
                    marker: (*p).to_string(),
                    strong: false,
                });
            }
        }
    }
    // `bug…:` — "**Bug (cause…):**" (same line, optional paren group)
    let mut from = 0;
    while let Some(i) = lower[from..].find("bug") {
        let i = from + i;
        if word_boundary(&lower, i, 3) && colon_after(&lower, i + 3) && !negated(&lower, i, &negs) {
            return Some(Hit {
                marker: "bug:".to_string(),
                strong: true,
            });
        }
        from = i + 3;
    }
    None
}

/// The fix phrase the agent said (chunk 5a), or `None` — quoted text dropped
/// and the pooled negation window applied, like `marker_hit`'s bug phrases.
/// An empty `packs` matches nothing.
pub fn fixed_hit(text: &str, packs: &[&Lang]) -> Option<String> {
    let lower = strip_quoted(text).to_lowercase();
    let negs: Vec<&str> = packs
        .iter()
        .flat_map(|p| p.negations.iter().copied())
        .collect();
    packs.iter().flat_map(|p| p.fixed).find_map(|p| {
        let i = lower.find(*p)?;
        (!negated(&lower, i, &negs)).then(|| (*p).to_string())
    })
}

/// The add-time language warning, or `None` when at most `FOREIGN_SHARE` of
/// the alphabetic chars in `text`, and in `title`, fall outside every accepted
/// `[lang] rows` script — one cited term (`ภ.ง.ด.3` in an English hand-off)
/// is not the row's language. Never a reject — one reject costs a whole
/// round — one warning line instead. Symbols (→, ≤) are not alphabetic;
/// accented Latin (é) is in the english script. An empty `rows` switches the
/// check off, mirroring `marker = []` — no accepted script would otherwise
/// make every letter foreign and garble the message.
pub fn row_language_check(cfg: &Config, title: Option<&str>, text: &str) -> Option<String> {
    if cfg.lang_rows.is_empty() {
        return None;
    }
    let packs: Vec<&Lang> = cfg.lang_rows.iter().filter_map(|n| by_name(n)).collect();
    // quoted text is a term cited verbatim (`ภาษี` in an English row), not
    // the row's language — only the prose outside code/quotes is judged
    let foreign = |s: &str| {
        let letters: Vec<char> = strip_quoted(s)
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect();
        let n = letters
            .iter()
            .filter(|&&c| !packs.iter().any(|p| in_script(p, c)))
            .count();
        n * FOREIGN_SHARE.1 > letters.len() * FOREIGN_SHARE.0
    };
    if !foreign(text) && !title.is_some_and(foreign) {
        return None;
    }
    let langs = packs.iter().map(|p| p.name).collect::<Vec<_>>().join("/");
    let langs = if langs == "english" {
        "English"
    } else {
        &langs
    };
    Some(format!(
        "row not in {langs} — write rows in {langs} from now on; cite a foreign term in `backticks`"
    ))
}

/// The share of foreign letters a row may carry before it reads as written
/// in another language (numerator, denominator): 1 in 5.
const FOREIGN_SHARE: (usize, usize) = (1, 5);

fn in_script(pack: &Lang, c: char) -> bool {
    pack.script.iter().any(|r| r.contains(&c))
}

/// Drop fenced code blocks, `inline code` spans and `>` quote lines — what is
/// left is the assistant's own prose, the only part that can report a problem.
fn strip_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.split_inclusive('\n') {
        // a ``` run anywhere opens/closes a fenced block when unpaired on
        // the line — the marker line itself is never prose worth matching
        if line.contains("```") {
            if line.matches("```").count() % 2 == 1 {
                in_fence = !in_fence;
            }
            continue;
        }
        if in_fence || line.trim_start().starts_with('>') {
            continue;
        }
        out.push_str(&strip_inline_code(line));
    }
    out
}

/// Drop `code` spans on one line; the tail after an unpaired tick is prose.
fn strip_inline_code(line: &str) -> String {
    let parts: Vec<&str> = line.split('`').collect();
    let mut out = String::with_capacity(line.len());
    for (i, part) in parts.iter().enumerate() {
        let tail_after_unpaired = parts.len().is_multiple_of(2) && i == parts.len() - 1;
        if i % 2 == 0 || tail_after_unpaired {
            out.push_str(part);
        }
    }
    out
}

fn word_boundary(s: &str, i: usize, len: usize) -> bool {
    let b = s.as_bytes();
    let left = i == 0 || !b[i - 1].is_ascii_alphanumeric();
    let right = b.get(i + len).is_none_or(|c| !c.is_ascii_alphanumeric());
    left && right
}

fn colon_after(s: &str, mut i: usize) -> bool {
    let b = s.as_bytes();
    while b.get(i).is_some_and(|c| *c == b' ' || *c == b'\t') {
        i += 1;
    }
    if b.get(i) == Some(&b'(') {
        // skip to the closing paren on this line
        while let Some(c) = b.get(i) {
            i += 1;
            if *c == b')' {
                break;
            }
            if *c == b'\n' {
                return false;
            }
        }
        while b.get(i).is_some_and(|c| *c == b' ' || *c == b'\t') {
            i += 1;
        }
    }
    // a colon before the line ends
    s[i..]
        .split('\n')
        .next()
        .is_some_and(|l| l.trim_end().ends_with(':'))
}

/// The ~15 chars before the match end in a negation word.
fn negated(lower: &str, i: usize, negations: &[&str]) -> bool {
    let before: String = lower[..i]
        .chars()
        .rev()
        .take(15)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let t = before.trim_end().to_lowercase();
    negations.iter().any(|n| {
        t.ends_with(n)
            && (n.chars().all(|c| !c.is_ascii_alphabetic())
                || t[..t.len() - n.len()]
                    .chars()
                    .last()
                    .is_none_or(|c| !c.is_alphabetic()))
    })
}

/// The sentence before the match (back to `.`/`!`/`?`/newline) holds a
/// conditional word — ASCII ones only as whole words, Thai ones anywhere
/// (Thai has no spaces between words).
// ponytail: 120-char window; a long Thai line with an unrelated ถ้า far back
// still cancels — a clause splitter if that ever shows up in replies
fn conditional(lower: &str, i: usize, conds: &[&str]) -> bool {
    let start = lower[..i].rfind(['.', '!', '?', '\n']).map_or(0, |j| j + 1);
    let sentence: String = lower[start..i].chars().rev().take(120).collect();
    let sentence: String = sentence.chars().rev().collect();
    conds.iter().any(|c| {
        if !c.is_ascii() {
            return sentence.contains(c);
        }
        sentence
            .match_indices(c)
            .any(|(j, _)| word_boundary(&sentence, j, c.len()))
    })
}
