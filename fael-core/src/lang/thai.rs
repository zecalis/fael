//! The Thai pack: its Stop-hook phrases, the words that cancel them and
//! its alphabet. A phrase added here gets its case in `fael-core/tests/lang.rs`.

use super::Lang;

pub(super) static TH: Lang = Lang {
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
        // the "root cause was" of a Thai reply: 51 of 17.8k assistant
        // messages in 14 days, about half naming a bug's cause
        "ต้นเหตุคือ",
        "สาเหตุคือ",
        "เจอต้นเหตุ",
        "พบต้นเหตุ",
    ],
    negations: &["ไม่", "จะ", "ถ้า", "อาจ"],
    risk_negations: &["ไม่"],
    conditionals: &["ถ้า", "หาก", "สมมติ"],
    // one range is the whole Thai block — the slice shape stays so EN/TH match
    #[allow(clippy::single_range_in_vec_init)]
    script: &['\u{0E00}'..='\u{0E7F}'],
};
