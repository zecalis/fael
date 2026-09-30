use crate::{Row, anchor};

/// Every word of the lowercased query `q` occurs in the row's text or title — lists show
/// titles, so they count — in any order: `login timeout` finds "timeout on
/// the login page". A blank `q` holds no word and matches every row.
pub(super) fn all_words(q: &str, r: &Row) -> bool {
    let text = r.text.to_lowercase();
    let title = r
        .title
        .as_deref()
        .map(str::to_lowercase)
        .unwrap_or_default();
    q.split_whitespace()
        .all(|w| text.contains(w) || title.contains(w))
}

/// Exact or under the directory (a zone) — an anchor's ref is opaque, never a zone.
/// Either side can be the zone: a row filed on `web/` or `web/**` covers `web/src/x.ts`.
pub(super) fn zone(q: &str, f: &str) -> bool {
    f == q || under(f, q) || covers(f, q)
}

/// `f` sits under directory `q` — never through an anchor, whose `/` is not a dir.
fn under(f: &str, q: &str) -> bool {
    anchor(q).is_none() && f.starts_with(q) && f.as_bytes().get(q.len()) == Some(&b'/')
}

/// The row's own file is a directory or a glob that takes in query `q` —
/// the scope for an area whose files do not exist yet.
fn covers(f: &str, q: &str) -> bool {
    if anchor(f).is_some() || anchor(q).is_some() {
        return false;
    }
    if f.contains(['*', '?', '[']) {
        return glob(f, q);
    }
    under(q, f.trim_end_matches('/'))
}

/// Same directory: both are paths (never anchors) with equal parent dirs.
/// Markdown never counts: a dir of plans/docs is a pile of unrelated
/// documents, a dir of code is a module (PLAN-fael-direction chunk 6) — so
/// editing one plan does not push rows filed against another.
pub(super) fn same_dir(q: &str, f: &str) -> bool {
    if anchor(q).is_some() || anchor(f).is_some() {
        return false;
    }
    if is_md(q) || is_md(f) {
        return false;
    }
    fn dir(s: &str) -> &str {
        s.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
    }
    dir(q) == dir(f)
}

/// Markdown by extension, case-insensitive (`README.MD` counts). Compares
/// bytes: a `str` slice at `len - 3` panics inside a multi-byte char.
pub(super) fn is_md(s: &str) -> bool {
    s.as_bytes()
        .get(s.len().saturating_sub(3)..)
        .is_some_and(|e| e.eq_ignore_ascii_case(b".md"))
}

/// Readers accept legacy spellings: `\` separators and a leading `./`.
pub(super) fn lenient(f: &str) -> String {
    let mut s = f.trim().replace('\\', "/");
    while let Some(rest) = s.strip_prefix("./") {
        s = rest.to_string();
    }
    s
}

pub(super) fn file_match(q: &str, f: &str) -> bool {
    if q.contains(['*', '?', '[']) {
        return glob(q, f);
    }
    zone(q, f)
}

/// Redis `KEYS` glob: `*` any run (including `:` and `/`), `?` one char, `[abc]` `[a-z]` `[^a]`, `\x` literal.
/// Memoises failed `(pattern, text)` positions, so it is O(|p|·|s|) — the
/// pattern is user input and `**********x` must not hang `find`.
pub fn glob(pattern: &str, s: &str) -> bool {
    fn m(p: &[char], s: &[char], i: usize, j: usize, dead: &mut [bool]) -> bool {
        let k = i * (s.len() + 1) + j;
        if dead[k] {
            return false;
        }
        let (p1, s1) = (&p[i..], &s[j..]);
        let hit = match p1.first() {
            None => s1.is_empty(),
            Some('*') => m(p, s, i + 1, j, dead) || (!s1.is_empty() && m(p, s, i, j + 1, dead)),
            Some('?') => !s1.is_empty() && m(p, s, i + 1, j + 1, dead),
            Some('[') if p1.len() > 2 && p1[2..].contains(&']') && !s1.is_empty() => {
                let end = 2 + p1[2..].iter().position(|&c| c == ']').unwrap();
                let (neg, set) = match p1[1] {
                    '^' => (true, &p1[2..end]),
                    _ => (false, &p1[1..end]),
                };
                let mut hit = false;
                let mut n = 0;
                while n < set.len() {
                    if n + 2 < set.len() && set[n + 1] == '-' {
                        hit |= (set[n]..=set[n + 2]).contains(&s1[0]);
                        n += 3;
                    } else {
                        hit |= set[n] == s1[0];
                        n += 1;
                    }
                }
                hit != neg && m(p, s, i + end + 1, j + 1, dead)
            }
            Some('\\') if p1.len() > 1 => {
                !s1.is_empty() && s1[0] == p1[1] && m(p, s, i + 2, j + 1, dead)
            }
            Some(&c) => !s1.is_empty() && s1[0] == c && m(p, s, i + 1, j + 1, dead),
        };
        dead[k] = !hit;
        hit
    }
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = s.chars().collect();
    let mut dead = vec![false; (p.len() + 1) * (s.len() + 1)];
    m(&p, &s, 0, 0, &mut dead)
}
