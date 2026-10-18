//! SHA-256 of a model file, streamed: a model can be hundreds of megabytes, and its hash is how a
//! known model is recognised and how a download is checked (`faces.md` §3.1).
//!
//! Ported from LightCraft `crates/faces/src/hash.rs` @ f34c7a8a (MIT OR Apache-2.0).

use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of everything `r` yields.
pub fn sha256_reader<R: Read>(mut r: R) -> io::Result<String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(buf.get(..n).unwrap_or(&[]));
    }
    Ok(hex(&h.finalize()))
}

/// Lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    sha256_reader(io::BufReader::new(std::fs::File::open(path)?))
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Whether `s` is a SHA-256 as this module writes it: 64 lowercase hexadecimal digits.
pub fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}
