//! The originals record on disk (`.dalica-originals/.dalica.json`, `data-model.md` §6): read,
//! changed and written back through `fs`'s atomic replace, never over a version Save did not read;
//! and the `md5:` digests that confirm a kept original is still the one recorded.

use std::io;
use std::path::{Path, PathBuf};

use dalica_core::{DateTime, Timestamp};
use dalica_fs::{Expect, Identity, originals as backup};
use dalica_sidecar::originals::{Entry, FILE_NAME, Originals};
use dalica_sidecar::{Field, Limits};

use super::{Error, Result};

/// The algorithm prefix of the digests Save writes.
pub const DIGEST_PREFIX: &str = "md5:";

/// The digest of `bytes` as the record writes it: `md5:` and 32 lowercase hex digits.
pub fn digest(bytes: &[u8]) -> String {
    format!("{DIGEST_PREFIX}{:x}", md5::compute(bytes))
}

/// Whether the bytes `reader` gives have the recorded digest `recorded`. An empty digest, or one
/// under an algorithm Dalica does not know, cannot confirm anything: `false`, and the backup is
/// left alone (`data-model.md` §6).
pub fn digest_matches(recorded: &str, reader: &mut dyn io::Read) -> io::Result<bool> {
    let Some(hex) = recorded.strip_prefix(DIGEST_PREFIX) else { return Ok(false) };
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(false);
    }
    let mut context = md5::Context::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => context.consume(buf.get(..n).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(format!("{:x}", context.finalize()).eq_ignore_ascii_case(hex))
}

/// A folder's originals record as Save read it, and the version it read.
pub(super) struct Record {
    pub path: PathBuf,
    pub doc: Originals,
    /// The identity of the file read; `None` when there was none (the next write makes it).
    identity: Option<Identity>,
}

impl Record {
    /// Reads the record of `folder`. No backup folder or no record: an empty one. A link in place
    /// of the backup folder or the record is refused. A damaged record is set aside as
    /// `.dalica.json.corrupt-<time>` when `set_aside` (a Save then starts a new record without
    /// writing over it), else refused.
    pub fn read(folder: &Path, now: Timestamp, set_aside: bool) -> Result<Record> {
        let dir = folder.join(backup::DIR_NAME);
        let path = dir.join(FILE_NAME);
        match dalica_fs::stat(&dir)? {
            None => return Ok(Record { path, doc: Originals::new(), identity: None }),
            Some(s) if s.is_file => return Err(Error::Record(format!("{} is a file, not a folder", dir.display()))),
            Some(_) => {}
        }
        let limits = Limits::default();
        let Some(read) = dalica_fs::read(&path, limits.max_bytes as u64)? else {
            return Ok(Record { path, doc: Originals::new(), identity: None });
        };
        match Originals::parse(&read.bytes, &limits) {
            Ok(doc) => Ok(Record { path, doc, identity: Some(read.identity) }),
            Err(_) if set_aside => {
                dalica_fs::set_aside(&path, &corrupt_tag(now))?;
                Ok(Record { path, doc: Originals::new(), identity: None })
            }
            Err(e) => Err(Error::Record(format!("{} is damaged: {e}", path.display()))),
        }
    }

    /// The entry for `name`: `None` when there is none; an entry Dalica cannot read is refused
    /// (it stays in the file).
    pub fn entry(&self, name: &str) -> Result<Option<Entry>> {
        match self.doc.entry(name) {
            Field::Absent => Ok(None),
            Field::Value(e) => Ok(Some(e)),
            Field::WrongType(_) => Err(Error::Record(format!("the entry for {name:?} is not one Dalica can read"))),
        }
    }

    /// Writes `entry` for `name` into the document (not yet to disk).
    pub fn set(&mut self, name: &str, entry: &Entry) -> Result<()> {
        self.doc.set(name, entry).map_err(|e| Error::Record(e.to_string()))
    }

    /// Puts `name`'s entry back to `before` (removed when `None`), in the document.
    pub fn put_back(&mut self, name: &str, before: Option<&Entry>) -> Result<()> {
        match before {
            Some(entry) => self.set(name, entry),
            None => self.doc.remove(name).map(|_| ()).map_err(|e| Error::Record(e.to_string())),
        }
    }

    /// Writes the document over the version read (or as a new file), atomically.
    pub fn store(&mut self) -> Result<()> {
        let expect = self.identity.map_or(Expect::Absent, Expect::Unchanged);
        self.identity = Some(dalica_fs::replace(&self.path, &self.doc.print(), expect)?);
        Ok(())
    }
}

/// `corrupt-<UTC time>Z`, the tag a damaged record is set aside under (as the scan does).
fn corrupt_tag(now: Timestamp) -> String {
    let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
    format!("corrupt-{stamp}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_are_md5_in_lowercase_hex() {
        assert_eq!(digest(b""), "md5:d41d8cd98f00b204e9800998ecf8427e");
        let d = digest(b"The quick brown fox jumps over the lazy dog");
        assert_eq!(d, "md5:9e107d9d372bb6826bd81d3542a419d6");
        assert!(digest_matches(&d, &mut &b"The quick brown fox jumps over the lazy dog"[..]).unwrap());
        assert!(digest_matches(&d.to_uppercase().replace("MD5", "md5"), &mut &b"The quick brown fox jumps over the lazy dog"[..]).unwrap());
        assert!(!digest_matches(&d, &mut &b"the quick brown fox"[..]).unwrap());
    }

    #[test]
    fn unknown_or_empty_digests_confirm_nothing() {
        for recorded in ["", "sha1:9e107d9d372bb6826bd81d3542a419d6", "md5:", "md5:xyz", "md5:9e107d9d372bb6826bd81d3542a419d"] {
            assert!(!digest_matches(recorded, &mut &b""[..]).unwrap(), "{recorded:?}");
        }
    }
}
