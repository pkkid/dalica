//! What to download, checked before anything is fetched: an https [`Url`], a local name that does
//! not come from the URL, a size cap, and an optional pinned size and SHA-256 (`faces.md` §3.1,
//! `security.md` §8.2). A [`FileSpec`] can only be made through [`FileSpec::new`], so every spec a
//! download sees has passed these checks.
//!
//! The name and pin checks are ported from LightCraft `crates/fetch/src/lib.rs` (`validate_files`)
//! at f34c7a8a (MIT OR Apache-2.0); the URL parser from its `http.rs` (`Url::parse`), https only.

use std::fmt;

/// The largest size cap a spec may set: 8 GiB, past any model or map file Dalica fetches.
pub const MAX_CAP: u64 = 8 << 30;
/// The longest URL accepted, in bytes.
pub const MAX_URL: usize = 8 * 1024;
/// The longest local name accepted, in bytes (with [`PART_SUFFIX`] it stays within 255).
pub const MAX_NAME: usize = 250;
/// The suffix of a partial download next to its final place.
pub const PART_SUFFIX: &str = ".part";

/// Why a spec was refused. Nothing was fetched or written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    /// The URL is not one Dalica fetches from.
    #[error("unusable address: {0}")]
    Url(String),
    /// The local name is not one plain, ordinary file name.
    #[error("{0:?} is not an ordinary file name")]
    Name(String),
    /// The cap is zero or larger than [`MAX_CAP`].
    #[error("the size cap {0} is not between 1 byte and {MAX_CAP} bytes")]
    Cap(u64),
    /// The pinned size is zero or past the cap.
    #[error("the pinned size {size} is not between 1 byte and the cap of {cap} bytes")]
    Size { size: u64, cap: u64 },
    /// The pinned SHA-256 is not 64 hexadecimal digits.
    #[error("the pinned SHA-256 is not 64 hexadecimal digits")]
    Sha256,
}

/// A parsed `https://` address. Only https: a plain-http address, user names and passwords in the
/// address, control characters and spaces are refused.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Url {
    host: String,
    port: u16,
    path: String,
}

impl Url {
    /// Parses `s`; the host is lowered, a fragment dropped, an empty path becomes `/`.
    pub fn parse(s: &str) -> Result<Url, SpecError> {
        let bad = |why: &str| SpecError::Url(why.to_string());
        if s.len() > MAX_URL {
            return Err(bad("the address is too long"));
        }
        if s.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(bad("the address contains spaces or control characters"));
        }
        let Some(rest) = strip_prefix_ignore_case(s, "https://") else {
            return Err(bad("only https:// addresses are fetched"));
        };
        let rest = rest.split('#').next().unwrap_or_default();
        let (authority, path) = match rest.find(['/', '?']) {
            Some(i) => (rest.get(..i).unwrap_or_default(), rest.get(i..).unwrap_or_default()),
            None => (rest, ""),
        };
        let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
        if authority.contains('@') {
            return Err(bad("user names and passwords in addresses are not sent"));
        }
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let end = v6.find(']').ok_or_else(|| bad("unterminated IPv6 address"))?;
            let host = v6.get(..end).unwrap_or_default();
            let after = v6.get(end.saturating_add(1)..).unwrap_or_default();
            if !after.is_empty() && !after.starts_with(':') {
                return Err(bad("bad host name"));
            }
            if host.is_empty() || !host.chars().all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.')) {
                return Err(bad("bad IPv6 address"));
            }
            (host.to_string(), after.strip_prefix(':'))
        } else {
            let (host, port) = match authority.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (authority, None),
            };
            if host.is_empty() || host.len() > 253 || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')) {
                return Err(bad("bad host name"));
            }
            (host.to_string(), port)
        };
        let port = match port {
            Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(|| bad("bad port"))?,
            None => 443,
        };
        Ok(Url { host: host.to_ascii_lowercase(), port, path })
    }

    /// The host name or address (an IPv6 address without its brackets), lower case.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The path and query, starting with `/`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The address without its query, for messages and logs (a query may hold a signed token).
    pub fn display(&self) -> String {
        format!("https://{}{}", self.authority(), self.path.split('?').next().unwrap_or("/"))
    }

    /// `host[:port]` as the `Host` header carries it (the port only when it is not 443).
    pub fn authority(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == 443 { host } else { format!("{host}:{}", self.port) }
    }
}

impl fmt::Display for Url {
    /// The whole address, query included.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "https://{}{}", self.authority(), self.path)
    }
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| s.get(prefix.len()..)).flatten()
}

/// One file to download: where from, the name it gets, its cap and its pins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSpec {
    url: Url,
    name: String,
    cap: u64,
    size: Option<u64>,
    sha256: Option<[u8; 32]>,
}

impl FileSpec {
    /// A spec for `url`, saved as `name`, never larger than `cap` bytes, optionally pinned to an
    /// exact `size` and a `sha256` (64 hexadecimal digits, either case). Everything is checked
    /// here; a refused spec never reaches a transport.
    pub fn new(url: &str, name: &str, cap: u64, size: Option<u64>, sha256: Option<&str>) -> Result<FileSpec, SpecError> {
        let url = Url::parse(url)?;
        if !ordinary_name(name) {
            return Err(SpecError::Name(name.to_string()));
        }
        if cap == 0 || cap > MAX_CAP {
            return Err(SpecError::Cap(cap));
        }
        if let Some(size) = size
            && (size == 0 || size > cap)
        {
            return Err(SpecError::Size { size, cap });
        }
        let sha256 = sha256.map(parse_sha256).transpose()?;
        Ok(FileSpec { url, name: name.to_string(), cap, size, sha256 })
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The local file name (never taken from the URL, a redirect or a response header).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name of the partial download: `<name>.part`.
    pub fn part_name(&self) -> String {
        format!("{}{PART_SUFFIX}", self.name)
    }

    /// The size cap in bytes.
    pub fn cap(&self) -> u64 {
        self.cap
    }

    /// The most bytes accepted: the pinned size, or the cap.
    pub fn limit(&self) -> u64 {
        self.size.unwrap_or(self.cap)
    }

    /// The pinned size, if any.
    pub fn size(&self) -> Option<u64> {
        self.size
    }

    /// The pinned SHA-256, if any.
    pub fn sha256(&self) -> Option<&[u8; 32]> {
        self.sha256.as_ref()
    }

    /// The pinned SHA-256 as 64 lower-case hexadecimal digits, if any.
    pub fn sha256_hex(&self) -> Option<String> {
        self.sha256.as_ref().map(|h| hex(h))
    }

    /// Whether a partial download of this file may be resumed: only with a pinned SHA-256, the one
    /// check that can tell bytes from an earlier run apart from bytes of another file.
    pub fn resumable(&self) -> bool {
        self.sha256.is_some()
    }
}

/// Lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_sha256(s: &str) -> Result<[u8; 32], SpecError> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return Err(SpecError::Sha256);
    }
    let mut out = [0u8; 32];
    let (pairs, _) = bytes.as_chunks::<2>();
    for (o, &[hi, lo]) in out.iter_mut().zip(pairs) {
        let digit = |b: u8| char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok());
        let (Some(hi), Some(lo)) = (digit(hi), digit(lo)) else {
            return Err(SpecError::Sha256);
        };
        *o = (hi << 4) | lo;
    }
    Ok(out)
}

/// One plain, ordinary file name: what `dalica-fs` accepts as a single component (no separator,
/// drive or stream prefix, Windows device name, temporary or Dalica name), and on top no control
/// or bidirectional-control characters, no characters Windows forbids, no leading dot (a hidden
/// file), no trailing dot or space, not a partial download's name, at most [`MAX_NAME`] bytes.
fn ordinary_name(name: &str) -> bool {
    dalica_fs::create::plain_name(name)
        && name.len() <= MAX_NAME
        && !name.starts_with('.')
        && !name.ends_with(['.', ' '])
        && !name.ends_with(PART_SUFFIX)
        && !name.contains(['*', '?', '"', '<', '>', '|'])
        && !name.chars().any(|c| c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}
