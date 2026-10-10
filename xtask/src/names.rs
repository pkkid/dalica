//! `cargo xtask names`: the reference product's name never appears in the repository (`AGENTS.md`,
//! "Reference research lives in `notes/`"). `AGENTS.md` is the one exempt file.
//!
//! The forbidden words are stored only as hashes (FNV-1a as a cheap prefilter, SHA-256 to confirm),
//! so this file passes its own check. Text is lower-cased and split into ASCII-alphanumeric runs;
//! every window of a run with a forbidden word's length is compared, so the word is found inside
//! longer identifiers and file names too.

use std::path::Path;
use std::process::Command;

/// A forbidden token: its byte length, FNV-1a 32 hash and SHA-256 (lower-case hex) of the
/// lower-case token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub len: usize,
    pub fnv: u32,
    pub sha256: &'static str,
}

impl Token {
    /// For tests: a token for a word we may write down.
    #[cfg(test)]
    pub fn of(word: &str) -> (usize, u32, String) {
        let w = word.to_ascii_lowercase();
        (w.len(), fnv1a(w.as_bytes()), sha256_hex(w.as_bytes()))
    }
}

/// The reference product's name.
pub const FORBIDDEN_WORDS: &[Token] =
    &[Token { len: 6, fnv: 0xdd0d_db4a, sha256: "7fcf284f3cd606b2f6a6c6bcb5adbe4d60af1a7c7654f2669cb8074c0cdfb682" }];

/// Files that may name the reference product.
pub const EXEMPT: &[&str] = &["AGENTS.md"];

pub fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(*b)).wrapping_mul(0x0100_0193))
}

/// SHA-256 (FIPS 180-4), small and dependency-free; inputs here are short words.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate().take(16) {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    sha256(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// A matcher over a set of tokens (the real list, or test words).
pub struct Matcher {
    tokens: Vec<(usize, u32, String)>,
}

impl Matcher {
    pub fn real(list: &[Token]) -> Matcher {
        Matcher { tokens: list.iter().map(|t| (t.len, t.fnv, t.sha256.to_string())).collect() }
    }

    #[cfg(test)]
    pub fn words(words: &[&str]) -> Matcher {
        Matcher { tokens: words.iter().map(|w| Token::of(w)).collect() }
    }

    /// Does `text` contain a forbidden token inside any alphanumeric run (case-insensitive)?
    /// Returns the run that contains it.
    pub fn find<'t>(&self, text: &'t str) -> Option<&'t str> {
        text.split(|c: char| !c.is_ascii_alphanumeric()).find(|run| self.run_matches(run))
    }

    /// Exact (whole-string) match, for file names and extensions.
    pub fn is(&self, s: &str) -> bool {
        let lower = s.to_ascii_lowercase();
        self.tokens.iter().any(|(len, fnv, sha)| lower.len() == *len && fnv1a(lower.as_bytes()) == *fnv && sha256_hex(lower.as_bytes()) == *sha)
    }

    fn run_matches(&self, run: &str) -> bool {
        let lower = run.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        self.tokens
            .iter()
            .any(|(len, fnv, sha)| *len > 0 && bytes.len() >= *len && bytes.windows(*len).any(|w| fnv1a(w) == *fnv && sha256_hex(w) == *sha))
    }
}

/// Files under version control or about to be (tracked, plus untracked files not ignored).
pub fn repo_files(root: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .output()
        .map_err(|e| format!("cannot run git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let mut files: Vec<String> = String::from_utf8_lossy(&out.stdout).split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect();
    files.sort();
    files.dedup();
    // Deleted-but-tracked files are listed by --cached; only check what exists.
    files.retain(|f| root.join(f).is_file());
    Ok(files)
}

/// `path:line: …` for every forbidden word in a file's path or (UTF-8) text.
pub fn check_files(root: &Path, files: &[String], matcher: &Matcher) -> Vec<String> {
    let mut out = Vec::new();
    for file in files {
        if EXEMPT.contains(&file.as_str()) {
            continue;
        }
        if matcher.find(file).is_some() {
            out.push(format!("{file}: the file's path contains a forbidden name"));
        }
        let Ok(bytes) = std::fs::read(root.join(file)) else { continue };
        let Ok(text) = std::str::from_utf8(&bytes) else { continue };
        for (i, line) in text.lines().enumerate() {
            if matcher.find(line).is_some() {
                out.push(format!("{file}:{}: contains a forbidden name (write \"the reference organizer\")", i + 1));
            }
        }
    }
    out
}

pub fn run(root: &Path) -> Result<(), String> {
    let files = repo_files(root)?;
    let problems = check_files(root, &files, &Matcher::real(FORBIDDEN_WORDS));
    for p in &problems {
        eprintln!("names: {p}");
    }
    if problems.is_empty() {
        println!("names: ok ({} files checked)", files.len());
        Ok(())
    } else {
        Err(format!("names: {} problem(s)", problems.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(fnv1a(b"abc"), 0x1a47_e90b);
    }

    #[test]
    fn matching_is_case_insensitive_and_finds_substrings() {
        let m = Matcher::words(&["zebra"]);
        assert_eq!(m.find("a ZEBRA crossing"), Some("ZEBRA"));
        assert_eq!(m.find("the .zebraoriginals folder"), Some("zebraoriginals"));
        assert_eq!(m.find("Zebra2Albums"), Some("Zebra2Albums"));
        assert_eq!(m.find("zebr a"), None);
        assert!(m.is("ZEBRA") && !m.is("zebras"));
    }

    fn tree(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let p = dir.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        (dir, files.iter().map(|(p, _)| p.to_string()).collect())
    }

    #[test]
    fn planted_word_fails_and_agents_md_is_exempt() {
        let m = Matcher::words(&["zebra"]);
        let (dir, files) = tree(&[("docs/a.md", "fine\nreads Zebra.ini files\n"), ("AGENTS.md", "the Zebra app")]);
        assert_eq!(check_files(dir.path(), &files, &m), ["docs/a.md:2: contains a forbidden name (write \"the reference organizer\")"]);
        let (dir, files) = tree(&[("AGENTS.md", "only here: zebra")]);
        assert!(check_files(dir.path(), &files, &m).is_empty());
    }

    #[test]
    fn forbidden_word_in_a_file_name_fails() {
        let m = Matcher::words(&["zebra"]);
        let (dir, files) = tree(&[("tests/fixtures/zebra-albums.txt", "nothing to see")]);
        assert_eq!(check_files(dir.path(), &files, &m), ["tests/fixtures/zebra-albums.txt: the file's path contains a forbidden name"]);
    }

    #[test]
    fn the_real_list_matches_the_name_in_agents_md() {
        // The exempt file names the product; the hashes must recognise it there, or the list is wrong.
        let text = std::fs::read_to_string(crate::workspace_root().join("AGENTS.md")).unwrap();
        assert!(Matcher::real(FORBIDDEN_WORDS).find(&text).is_some());
    }
}
