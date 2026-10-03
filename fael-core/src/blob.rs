//! Git blob ids of file content, the way `git hash-object` computes them.
//! Core only hashes bytes — reading the disk is the binary's job
//! (`fael/src/filehash.rs`).

use sha1::{Digest, Sha1};

/// The 12-hex prefix of the git blob id for `bytes`, hashed as if stored with
/// LF: CRLF is normalised to LF first, so the same file yields the same id on
/// a Windows checkout and a mac one — `git hash-object <f> | cut -c1-12` in a
/// repo that stores LF.
pub fn blob_id(bytes: &[u8]) -> String {
    let lf = normalize_lf(bytes);
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", lf.len()).as_bytes());
    h.update(&lf);
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    hex[..12].to_string()
}

/// `\r\n` → `\n`. A lone `\r` is left alone (no autocrlf case maps it).
fn normalize_lf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{blob_id, normalize_lf};

    #[test]
    fn blob_id_matches_git() {
        // FIPS 180-1 vector: sha1("blob 3\0abc") = f2ba8f84ab5c…
        assert_eq!(blob_id(b"abc"), "f2ba8f84ab5c");
    }

    #[test]
    fn crlf_hashes_like_lf() {
        assert_eq!(blob_id(b"a\r\nb\n"), blob_id(b"a\nb\n"));
        assert_eq!(normalize_lf(b"a\rb"), b"a\rb");
    }
}
