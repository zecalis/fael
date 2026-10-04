//! Git blob ids of file content, the way `git hash-object` computes them.
//! Core only hashes bytes — reading the disk is the binary's job
//! (`fael/src/filehash.rs`).

use sha1::{Digest, Sha1};
use std::borrow::Cow;
use std::fmt::Write;

/// How far git looks for a NUL before it calls a file binary.
const BINARY_PROBE: usize = 8000;

/// The 12-hex prefix of the git blob id for `bytes`. A text file is hashed as
/// if stored with LF — CRLF is normalised first, so the same file yields the
/// same id on a Windows checkout and a mac one, and equals
/// `git hash-object <f> | cut -c1-12` in a repo that stores LF. A binary file
/// (a NUL in the first 8000 bytes, git's own test) is hashed raw: git never
/// rewrites its line endings, so neither do we.
pub fn blob_id(bytes: &[u8]) -> String {
    let lf = normalize_lf(bytes);
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", lf.len()).as_bytes());
    h.update(&lf);
    let mut hex = String::with_capacity(40);
    for b in h.finalize() {
        let _ = write!(hex, "{b:02x}");
    }
    hex.truncate(12);
    hex
}

/// `\r\n` → `\n` in a text file, borrowed when there is nothing to change. A
/// lone `\r` is left alone (no autocrlf case maps it); binary is untouched.
fn normalize_lf(bytes: &[u8]) -> Cow<'_, [u8]> {
    let binary = bytes.iter().take(BINARY_PROBE).any(|&b| b == 0);
    if binary || !bytes.windows(2).any(|w| w == b"\r\n") {
        return Cow::Borrowed(bytes);
    }
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
    Cow::Owned(out)
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
        assert_eq!(&*normalize_lf(b"a\rb"), b"a\rb");
    }

    #[test]
    fn binary_keeps_its_crlf() {
        assert_ne!(blob_id(b"\0a\r\nb"), blob_id(b"\0a\nb"));
    }
}
