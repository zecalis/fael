//! The English pack: its Stop-hook phrases, the words that cancel them and
//! its alphabet. A phrase added here gets its case in `fael-core/tests/lang.rs`.

use super::Lang;

pub(super) static EN: Lang = Lang {
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
