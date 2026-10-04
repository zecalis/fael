//! Git blob ids of file content, the way `git hash-object` computes them.
//! Core only hashes a stream — opening the disk is the binary's job
//! (`fael/src/filehash.rs`).

use sha1::{Digest, Sha1};
use std::fmt::Write;
use std::io::{self, Read, Seek};

/// How far git looks for a NUL before it calls a file binary.
const BINARY_PROBE: u64 = 8000;
/// Read size: memory stays this big whatever the file is.
const CHUNK: usize = 64 * 1024;

/// The 12-hex prefix of the git blob id of everything `r` yields, or `None`
/// when it is longer than `max` bytes (or changed length between the two
/// passes). A text file is hashed as if stored with LF — CRLF is normalised,
/// so the same file yields the same id on a Windows checkout and a mac one,
/// and equals `git hash-object <f> | cut -c1-12` in a repo that stores LF. A
/// binary file (a NUL in the first 8000 bytes, git's own test) is hashed raw:
/// git never rewrites its line endings, so neither do we.
///
/// Two passes, never the whole file in memory: git's header carries the
/// normalised length, so the first pass counts it and the second hashes.
pub fn blob_id_stream<R: Read + Seek>(r: &mut R, max: u64) -> io::Result<Option<String>> {
    r.rewind()?;
    let mut buf = vec![0u8; CHUNK];
    let (mut total, mut pairs, mut prev_cr, mut binary) = (0u64, 0u64, false, false);
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        let probe = BINARY_PROBE.saturating_sub(total).min(n as u64) as usize;
        binary |= chunk[..probe].contains(&0);
        for &b in chunk {
            pairs += u64::from(b == b'\n' && prev_cr);
            prev_cr = b == b'\r';
        }
        total += n as u64;
        if total > max {
            return Ok(None);
        }
    }
    let crlf = !binary && pairs > 0;
    r.rewind()?;
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", if crlf { total - pairs } else { total }).as_bytes());
    let (mut seen, mut pending_cr) = (0u64, false);
    let mut out = Vec::with_capacity(if crlf { CHUNK } else { 0 });
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        seen += n as u64;
        if seen > total {
            return Ok(None);
        }
        if !crlf {
            h.update(&buf[..n]);
            continue;
        }
        out.clear();
        for &b in &buf[..n] {
            if pending_cr {
                pending_cr = false;
                if b == b'\n' {
                    out.push(b'\n');
                    continue;
                }
                out.push(b'\r');
            }
            if b == b'\r' {
                pending_cr = true;
            } else {
                out.push(b);
            }
        }
        h.update(&out);
    }
    if pending_cr {
        h.update(b"\r"); // a lone `\r` last in the file stays
    }
    if seen != total {
        return Ok(None);
    }
    let mut hex = String::with_capacity(40);
    for b in h.finalize() {
        let _ = write!(hex, "{b:02x}");
    }
    hex.truncate(12);
    Ok(Some(hex))
}

#[cfg(test)]
mod tests {
    use super::{CHUNK, blob_id_stream};
    use std::io::Cursor;

    fn id(bytes: &[u8]) -> String {
        blob_id_stream(&mut Cursor::new(bytes), u64::MAX)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn blob_id_matches_git() {
        // FIPS 180-1 vector: sha1("blob 3\0abc") = f2ba8f84ab5c…
        assert_eq!(id(b"abc"), "f2ba8f84ab5c");
    }

    #[test]
    fn crlf_hashes_like_lf() {
        assert_eq!(id(b"a\r\nb\n"), id(b"a\nb\n"));
        assert_ne!(id(b"a\rb"), id(b"a\nb")); // a lone \r is left alone
        assert_eq!(id(b"a\r"), id(b"a\r")); // ...even last in the file
        assert_ne!(id(b"a\r"), id(b"a"));
        assert_ne!(id(b"a\r\r\nb"), id(b"a\nb")); // only the pair collapses, the first \r stays
    }

    #[test]
    fn binary_keeps_its_crlf() {
        assert_ne!(id(b"\0a\r\nb"), id(b"\0a\nb"));
    }

    #[test]
    fn crlf_across_a_chunk_boundary_is_one_pair() {
        let mut crlf = vec![b'a'; CHUNK - 1];
        crlf.extend_from_slice(b"\r\nb\r\n");
        let lf = [&crlf[..CHUNK - 1], b"\nb\n"].concat();
        assert_eq!(id(&crlf), id(&lf));
    }

    #[test]
    fn longer_than_max_is_none() {
        let mut r = Cursor::new(vec![b'x'; 11]);
        assert_eq!(blob_id_stream(&mut r, 10).unwrap(), None);
        assert!(blob_id_stream(&mut r, 11).unwrap().is_some());
    }
}
