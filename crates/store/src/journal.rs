//! The index journal, `index.log` (`architecture.md` §4, `security.md` §4.2): one record per line,
//! `<crc32 as 8 hex digits> <json>\n`. The first record is a header (`{"journal": 1, "base": N}`:
//! the format version, and how many ops the snapshot already holds); every other record is one
//! applied [`Op`]. Each append is synced to disk. Sketch batches ([`Op::SetSketches`]) are records
//! like any other: replay restores them, and a torn batch is lost whole while the ones before it
//! are kept. M6's ops (captions, tags, hidden flags, recipes, dates, folder info, synced hashes)
//! are records the same way; replay applies them in order.
//!
//! Reading stops cleanly at the first line that is torn (no newline), fails its checksum or does
//! not parse: everything before it is kept, nothing after it is trusted.

use std::path::{Path, PathBuf};

use dalica_fs::{Appender, Expect};
use dalica_library::Op;
use serde::{Deserialize, Serialize};

use crate::{Error, Result, crc32};

/// The journal's file name in the library directory.
pub const FILE: &str = "index.log";
/// The format version this crate writes and reads.
pub const VERSION: u64 = 1;
/// The largest record (`security.md` §4.2).
pub const MAX_RECORD: usize = 16 << 20;
/// The journal never grows past this; a snapshot resets it long before.
const CAP: u64 = 4 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub journal: u64,
    /// Ops numbered `1..=base` are in the snapshot; this file's first op is `base + 1`.
    pub base: u64,
}

/// One line: checksum, space, JSON, newline.
pub(crate) fn encode(json: &[u8]) -> Vec<u8> {
    let mut line = format!("{:08x} ", crc32(json)).into_bytes();
    line.extend_from_slice(json);
    line.push(b'\n');
    line
}

/// The JSON of one line (without its newline), if its checksum matches.
pub(crate) fn decode(line: &[u8]) -> std::result::Result<&[u8], String> {
    let (crc, json) = match (line.get(..8), line.get(8), line.get(9..)) {
        (Some(crc), Some(b' '), Some(json)) => (crc, json),
        _ => return Err("not a checksummed record".into()),
    };
    let want = std::str::from_utf8(crc).ok().and_then(|c| u32::from_str_radix(c, 16).ok()).ok_or("bad checksum field")?;
    if crc32(json) != want {
        return Err("checksum mismatch".into());
    }
    Ok(json)
}

/// Why reading stopped before the end of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// The last line has no newline: a write was cut off.
    Torn,
    /// Record `index` (0 = the header) is damaged or unreadable.
    Corrupt { index: u64, reason: String },
}

/// What a journal file holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Contents {
    pub header: Option<Header>,
    pub ops: Vec<Op>,
    /// Where each valid record ends (index 0: the header), so a prefix can be cut at any record.
    pub ends: Vec<usize>,
    /// Bytes of the file that are whole, valid records.
    pub valid_len: usize,
    pub stop: Option<Stop>,
}

/// Reads a journal's bytes as far as they are valid.
pub fn read(bytes: &[u8]) -> Contents {
    let mut out = Contents { header: None, ops: Vec::new(), ends: Vec::new(), valid_len: 0, stop: None };
    let mut pos = 0;
    let mut index = 0u64;
    while pos < bytes.len() {
        let rest = bytes.get(pos..).unwrap_or_default();
        let Some(nl) = rest.iter().position(|&b| b == b'\n') else {
            out.stop = Some(Stop::Torn);
            break;
        };
        let line = rest.get(..nl).unwrap_or_default();
        let corrupt = |reason: String| Some(Stop::Corrupt { index, reason });
        if line.len() > MAX_RECORD + 9 {
            out.stop = corrupt(format!("a record of {} bytes", line.len()));
            break;
        }
        let json = match decode(line) {
            Ok(j) => j,
            Err(reason) => {
                out.stop = corrupt(reason);
                break;
            }
        };
        if index == 0 {
            match serde_json::from_slice::<Header>(json) {
                Ok(h) if h.journal == VERSION => out.header = Some(h),
                Ok(h) => {
                    out.stop = corrupt(format!("journal version {} (this Dalica reads {VERSION})", h.journal));
                    break;
                }
                Err(e) => {
                    out.stop = corrupt(format!("header: {e}"));
                    break;
                }
            }
        } else {
            match serde_json::from_slice::<Op>(json) {
                Ok(op) => out.ops.push(op),
                Err(e) => {
                    out.stop = corrupt(e.to_string());
                    break;
                }
            }
        }
        pos += nl + 1;
        out.valid_len = pos;
        out.ends.push(pos);
        index += 1;
    }
    out
}

/// The open journal: appends records and knows how many ops it holds.
#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    file: Appender,
    base: u64,
    ops: u64,
}

impl Journal {
    /// Replaces `path` with a journal holding only its header (`base`) and opens it.
    pub fn create(path: &Path, base: u64) -> Result<Journal> {
        let header = serde_json::to_vec(&Header { journal: VERSION, base }).map_err(|e| Error::Json(e.to_string()))?;
        write_whole(path, &encode(&header))?;
        Ok(Journal { path: path.to_path_buf(), file: Appender::open(path, CAP)?, base, ops: 0 })
    }

    /// Replaces `path` with exactly `bytes` (a valid prefix: header and `ops` records) and opens it.
    pub(crate) fn resume(path: &Path, bytes: &[u8], base: u64, ops: u64, rewrite: bool) -> Result<Journal> {
        if rewrite {
            write_whole(path, bytes)?;
        }
        Ok(Journal { path: path.to_path_buf(), file: Appender::open(path, CAP)?, base, ops })
    }

    /// Appends `op` (synced). Its number is returned.
    pub fn append(&mut self, op: &Op) -> Result<u64> {
        let json = serde_json::to_vec(op).map_err(|e| Error::Json(e.to_string()))?;
        if json.len() > MAX_RECORD {
            return Err(Error::RecordTooLarge(json.len()));
        }
        self.file.append(&encode(&json))?;
        self.ops += 1;
        Ok(self.seq())
    }

    /// The number of the last op written: everything up to it is in the snapshot or this file.
    pub fn seq(&self) -> u64 {
        self.base + self.ops
    }

    /// Ops in this file (since the last snapshot).
    pub fn len(&self) -> u64 {
        self.ops
    }

    pub fn is_empty(&self) -> bool {
        self.ops == 0
    }

    pub fn bytes(&self) -> u64 {
        self.file.size()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Atomically replaces (or creates) `path` with `bytes`. The library lock keeps every other
/// process out, so the version on disk is the one to replace.
pub(crate) fn write_whole(path: &Path, bytes: &[u8]) -> Result<()> {
    let expect = match dalica_fs::identity(path)? {
        Some(id) => Expect::Unchanged(id),
        None => Expect::Absent,
    };
    dalica_fs::replace(path, bytes, expect)?;
    Ok(())
}
