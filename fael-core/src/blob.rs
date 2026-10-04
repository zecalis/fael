//! Git blob ids of file content, the way `git hash-object` computes them.
//! Core only hashes a stream — opening the disk is the binary's job
//! (`fael/src/filehash.rs`).

use sha1::{Digest, Sha1};
use std::fmt::Write;
use std::io::{self, Read, Seek, SeekFrom};

/// How far git looks for a NUL before it calls a file binary.
const BINARY_PROBE: u64 = 8000;
/// Read size: memory stays this big whatever the file is.
const CHUNK: usize = 64 * 1024;

/// The 12-hex prefix of the git blob id of everything `r` yields, or `None`
/// when it is longer than `max` bytes (or changed length while it was read).
/// A text file is hashed as if stored with LF — CRLF is normalised, so the
/// same file yields the same id on a Windows checkout and a mac one. A binary
/// file (a NUL in the first 8000 bytes) is hashed raw: its line endings are
/// never rewritten.
///
/// This is the rule fael implements, not git's whole CRLF heuristic: git also
/// reads a lone `\r`, or a NUL past byte 8000, as "binary" and leaves such a
/// file unconverted, where this normalises it. Only text with no lone `\r`
/// and no late NUL matches `git hash-object | cut -c1-12` in a repo that
/// stores LF. Both sides of every fael comparison use this function, so the
/// difference never reads as a change.
///
/// Never the whole file in memory. The length comes from a seek, so a file
/// with no CRLF pair (the common case) is one pass; a text file that has one
/// takes a second, because git's header carries the normalised length.
pub fn blob_id_stream<R: Read + Seek>(r: &mut R, max: u64) -> io::Result<Option<String>> {
    let len = r.seek(SeekFrom::End(0))?;
    if len > max {
        return Ok(None);
    }
    r.rewind()?;
    let mut buf = vec![0u8; CHUNK];
    let mut h = header(len);
    let (mut seen, mut pairs, mut prev_cr, mut binary) = (0u64, 0u64, false, false);
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        let probe = BINARY_PROBE.saturating_sub(seen).min(n as u64) as usize;
        binary |= chunk[..probe].contains(&0);
        for &b in chunk {
            pairs += u64::from(b == b'\n' && prev_cr);
            prev_cr = b == b'\r';
        }
        seen += n as u64;
        if seen > len {
            return Ok(None);
        }
        h.update(chunk);
    }
    if seen != len {
        return Ok(None);
    }
    if binary || pairs == 0 {
        return Ok(Some(hex12(h)));
    }
    // CRLF text: hash again with each pair collapsed
    r.rewind()?;
    let mut h = header(len - pairs);
    let (mut seen, mut pending_cr) = (0u64, false);
    let mut out = Vec::with_capacity(CHUNK);
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        seen += n as u64;
        if seen > len {
            return Ok(None);
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
    Ok((seen == len).then(|| hex12(h)))
}

/// Git's blob header for a body of `len` bytes.
fn header(len: u64) -> Sha1 {
    let mut h = Sha1::new();
    h.update(format!("blob {len}\0").as_bytes());
    h
}

fn hex12(h: Sha1) -> String {
    let mut hex = String::with_capacity(40);
    for b in h.finalize() {
        let _ = write!(hex, "{b:02x}");
    }
    hex.truncate(12);
    hex
}

#[cfg(test)]
mod tests {
    use super::{CHUNK, blob_id_stream};
    use std::io::{self, Cursor, Read, Seek, SeekFrom};

    /// One byte per `read`, the worst case for a chunk boundary.
    struct Trickle(Cursor<Vec<u8>>);
    impl Read for Trickle {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            let n = 1.min(b.len());
            self.0.read(&mut b[..n])
        }
    }
    impl Seek for Trickle {
        fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
            self.0.seek(p)
        }
    }

    /// Says its length is `claimed` while it yields `data`: a file that grew
    /// or shrank between the seek and the read.
    struct Lying {
        data: Cursor<Vec<u8>>,
        claimed: u64,
    }
    impl Read for Lying {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.data.read(b)
        }
    }
    impl Seek for Lying {
        fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
            match p {
                SeekFrom::End(0) => Ok(self.claimed),
                p => self.data.seek(p),
            }
        }
    }

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
        assert_eq!(id(b"a\r"), "db0a4d316ae8"); // ...even last in the file: sha1("blob 2\0a\r")
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

    #[test]
    fn one_byte_reads_hash_the_same() {
        for body in [&b"a\r\nb\r\n\r"[..], b"plain\nlf\n", b"\0a\r\nb"] {
            let slow = blob_id_stream(&mut Trickle(Cursor::new(body.to_vec())), u64::MAX);
            assert_eq!(slow.unwrap().unwrap(), id(body));
        }
    }

    #[test]
    fn a_file_that_changed_length_is_none() {
        for (body, claimed) in [
            (&b"abcdef"[..], 3),
            (b"abc", 6),
            (b"a\r\nb\r\n", 3),
            (b"a\r\nb\r\n", 9),
        ] {
            let mut r = Lying {
                data: Cursor::new(body.to_vec()),
                claimed,
            };
            assert_eq!(
                blob_id_stream(&mut r, u64::MAX).unwrap(),
                None,
                "{body:?} as {claimed}"
            );
        }
    }
}
