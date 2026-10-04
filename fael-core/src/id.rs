//! ids and clocks: ULID, RFC 3339 timestamps, writer id — std only apart from the OS RNG and sha256.

use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A ULID: 48-bit ms timestamp + 80 random bits, 26 chars of Crockford base32.
pub fn ulid() -> String {
    ulid_at(now_ms())
}

// ponytail: no per-ms monotonic counter — two rows in the same ms from one process sort randomly;
// add the counter if causal order inside one ms ever matters
pub fn ulid_at(ms: u64) -> String {
    let mut rnd = [0u8; 16];
    getrandom::fill(&mut rnd[6..]).expect("OS random source");
    let n = ((ms as u128 & 0xFFFF_FFFF_FFFF) << 80) | u128::from_be_bytes(rnd);
    (0..26)
        .rev()
        .map(|i| CROCKFORD[((n >> (5 * i)) & 31) as usize] as char)
        .collect()
}
/// The ms timestamp inside a ULID (the first 10 chars are 48-bit ms
/// big-endian Crockford base32). `None` on a short or non-ULID id — the
/// `[Shipped]` doctor check falls back to the row's `ts` then.
pub fn ulid_ms(id: &str) -> Option<u64> {
    if id.len() < 10 {
        return None;
    }
    let mut ms: u64 = 0;
    for c in id.chars().take(10) {
        ms = ms.checked_mul(32)?.checked_add(crockford_val(c)? as u64)?;
    }
    Some(ms)
}

fn crockford_val(c: char) -> Option<u8> {
    // Crockford base32 skips I, L, O and U — the ranges below leave those
    // out, so a wrong letter is None, never a shifted value.
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'A'..='H' => Some(c as u8 - b'A' + 10),
        'J'..='K' => Some(c as u8 - b'J' + 18),
        'M' => Some(20),
        'N' => Some(21),
        'P'..='T' => Some(c as u8 - b'P' + 22),
        'V'..='Z' => Some(c as u8 - b'V' + 27),
        'a'..='h' => Some(c as u8 - b'a' + 10),
        'j'..='k' => Some(c as u8 - b'j' + 18),
        'm' => Some(20),
        'n' => Some(21),
        'p'..='t' => Some(c as u8 - b'p' + 22),
        'v'..='z' => Some(c as u8 - b'v' + 27),
        _ => None,
    }
}

/// An id-shaped token (PLAN-fael-id-refs contract): 8..=26 chars, starting
/// `0`, every char Crockford base32 (no I L O U, either case). 8 is the floor
/// because `Abbrev::short` never prints fewer — a shorter token is prose
/// (e.g. `0123`), not something fael ever printed. The first char is `0`
/// because a 128-bit ULID keeps its top two bits zero: it only reaches `1`
/// when the 48-bit ms passes 2^45, ~year 3084. (An earlier `01`-only check
/// stopped recognising every id from 2039 on, when the second char turns
/// `2` — the guard silently disabled itself.) At least one letter: an
/// all-digit token is a number in prose (a tax id `0107544000108`), never a
/// fael id — a ULID's time part holds a letter from ~1980 on (`01M…` today).
///
/// Legacy fapony `mug…` ids are outside this shape: `resolve` still finds them
/// by exact id/prefix, but prose scanning ignores their shape (short lowercase
/// runs would flag ordinary words as phantoms).
pub fn looks_like_id(tok: &str) -> bool {
    (8..=26).contains(&tok.len())
        && tok.as_bytes().first() == Some(&b'0')
        && tok.chars().all(|c| crockford_val(c).is_some())
        && !tok.bytes().all(|b| b.is_ascii_digit())
}
/// present: the stop hook anchors recency at a transcript birthtime with ms
/// precision, and a whole-second row filed just before the session start
/// would otherwise read as newer.
pub fn rfc3339(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, sod) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil_from_days
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        sod / 3600,
        sod / 60 % 60,
        sod % 60,
        ms % 1000,
    )
}

/// `"2026-09-25T10:00:01Z"` (or `+00:00`, optional `.frac`) → unix ms.
/// `None` = unparsable — the hook's numeric recency compare runs on this, so
/// a row filed a second before the session start never reads as newer.
pub fn ts_ms(s: &str) -> Option<i64> {
    let (dt, off) = match s.strip_suffix('Z').or_else(|| s.strip_suffix('z')) {
        Some(d) => (d, 0i64),
        None => {
            let i = s.rfind(['+', '-'])?;
            if i == 0 || s.as_bytes().get(i - 1) == Some(&b'T') {
                return None; // the `-` of the date, not a zone
            }
            let (d, z) = s.split_at(i);
            let (h, m) = z[1..].split_once(':')?;
            let secs: i64 = h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60;
            (d, if z.starts_with('-') { -secs } else { secs })
        }
    };
    let (d, t) = dt.split_once('T')?;
    let mut d = d.split('-');
    let (y, mo, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let mut t = t.split(':');
    let (h, mi): (i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    let ssec = t.next()?;
    let (sec_s, frac_s) = match ssec.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (ssec, None),
    };
    let sec: i64 = sec_s.parse().ok()?;
    let frac_ms: i64 = frac_s
        .map(|f| {
            let digits: String = f.chars().take_while(|c| c.is_ascii_digit()).collect();
            format!("{:0<3}", &digits[..digits.len().min(3)])
                .parse()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&day) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    Some(
        days_from_civil(y, mo, day) * 86_400_000
            + (h * 3600 + mi * 60 + sec - off) * 1000
            + frac_ms,
    )
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9).rem_euclid(12);
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Does an issue `to` value route to `reader` (a writer id)? Match = the
/// full writer id or its name part (before the last `-<hash>`); no prefix
/// match (`dela` never matches `delamind-d88f`). Case-insensitive — write
/// lowercases already, this covers hand-written rows.
pub fn to_matches(to: &str, reader: &str) -> bool {
    if to.is_empty() || reader.is_empty() {
        return false;
    }
    let (to, reader) = (to.to_lowercase(), reader.to_lowercase());
    if to == reader {
        return true;
    }
    reader.rsplit_once('-').is_some_and(|(name, _)| to == name)
}

/// `<slug of name>-<4 hex of sha256(lowercase email)>`; no email → hash `host` instead.
/// The email itself never reaches the repo — only the hash.
pub fn writer_id(name: &str, email: Option<&str>, host: &str) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let slug = if slug.is_empty() { "anon" } else { slug };
    let seed = email.map_or_else(|| host.to_string(), str::to_lowercase);
    let h = Sha256::digest(seed.as_bytes());
    format!("{slug}-{:02x}{:02x}", h[0], h[1])
}

#[cfg(test)]
mod tests {
    #[test]
    fn ts_ms_leading_sign_is_none_not_panic() {
        assert_eq!(super::ts_ms("-1"), None);
        assert_eq!(super::ts_ms("+x"), None);
    }

    #[test]
    fn ulid_ms_round_trips_ulid_at() {
        for ms in [0u64, 1, 1_000, 1_758_864_000_000] {
            let id = super::ulid_at(ms);
            assert_eq!(super::ulid_ms(&id), Some(ms), "{id}");
        }
    }

    #[test]
    fn ulid_ms_rejects_short_and_bad_chars() {
        assert_eq!(super::ulid_ms(""), None);
        assert_eq!(super::ulid_ms("01J8ZQ3K4"), None); // 9 chars
        assert_eq!(super::ulid_ms("01J8ZQ3K4*XXXXXXXXXXXXXXXX"), None); // * not Crockford
        assert_eq!(super::ulid_ms("01J8ZQ3K4IXXXXXXXXXXXXXXXX"), None); // I excluded
    }
}
