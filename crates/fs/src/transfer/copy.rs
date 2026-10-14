//! A file copied into a new file and verified byte for byte before it gets its name: the first
//! phase of a cross-volume move, and a copy.
//!
//! Adapted from LightCraft `crates/engine/src/import_move.rs` @ b7091805 (MIT OR Apache-2.0):
//! `copy_verified` (a new file, synced, the source's modification time, compared with the source,
//! removed on any failure), `same_bytes` and `fill`. Here the copy is written under a temporary
//! name and committed by a rename that never replaces, relative to held folders.

use std::collections::hash_map::RandomState;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::hash::{BuildHasher as _, Hasher as _};
use std::io::{self, Read, Seek as _, SeekFrom, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use super::{Outcome, Renamed, check_file, rename_free};
use crate::atomic::Identity;
use crate::dirfd::{Entry, Folder, Kind};
use crate::faults::{self, Fault, points};
use crate::{Error, Result, io_err};

/// Bytes read at a time while copying and comparing.
const CHUNK: usize = 1 << 20;

/// Copies `name` in `src` (identity `expect`) to `to` in `dest` (see [`super::copy_file`]).
pub(super) fn place(src: &Folder, name: &OsStr, expect: Identity, dest: &Folder, to: &OsStr) -> Result<Outcome<Identity>> {
    faults::at(points::MOVE_CHECK, "stat", &src.join(name))?;
    check_file(src, name, expect)?;
    let entry = src.entry(name)?.ok_or_else(|| Error::Changed(src.join(name)))?;
    let mut input = open_source(src, name, expect)?;
    let modified = input.metadata().and_then(|m| m.modified()).ok();

    // 1. A temporary file, written, given the source's time and permissions, made durable.
    faults::at(points::COPY_TEMP, "create", dest.path())?;
    let (tmp, mut out) = create_temp(dest, to)?;
    let tmp_path = dest.join(&tmp);
    let cleanup = |e: Error| {
        if !matches!(e, Error::Crashed(_)) {
            let _ = dest.remove(&tmp);
        }
        e
    };
    let first = stream(&mut input, &mut out, expect.size(), points::COPY_WRITE, &src.join(name), &tmp_path).map_err(cleanup)?;
    keep_attributes(&out, &entry, modified);
    faults::at(points::COPY_FSYNC, "fsync", &tmp_path).map_err(cleanup)?;
    out.sync_all().map_err(|e| cleanup(io_err("fsync", &tmp_path)(e)))?;
    flip_switch(&mut out, first, &tmp_path).map_err(cleanup)?;
    drop(out);
    // By name, not through the write-only handle (which may not read attributes on Windows).
    let copied = match dest.entry(&tmp).map_err(cleanup)? {
        Some(e) if e.kind == Kind::File => e.identity,
        _ => return Err(cleanup(Error::Changed(tmp_path))),
    };

    // 2. Byte for byte against the source, before it gets a name.
    faults::at(points::COPY_VERIFY, "verify", &tmp_path).map_err(cleanup)?;
    if !same_bytes(src, name, expect, dest, &tmp).map_err(cleanup)? {
        return Err(cleanup(Error::VerifyFailed(dest.join(to))));
    }

    // 3. The name, never replacing a file.
    faults::at(points::COPY_COMMIT, "rename", &dest.join(to)).map_err(cleanup)?;
    match rename_free(dest, &tmp, dest, to).map_err(cleanup)? {
        Renamed::Done => {}
        Renamed::Taken => {
            let _ = dest.remove(&tmp);
            return Ok(Outcome::Exists);
        }
        Renamed::CrossVolume => return Err(cleanup(io_err("rename", &dest.join(to))(io::ErrorKind::CrossesDevices.into()))),
    }
    faults::at(points::COPY_FSYNC_DIR, "fsync", dest.path())?;
    dest.sync()?;
    faults::at(points::MOVE_VERIFY, "verify", &dest.join(to))?;
    match dest.entry(to)? {
        Some(e) if e.kind == Kind::File && e.identity == copied => Ok(Outcome::Done(copied)),
        _ => Err(Error::VerifyFailed(dest.join(to))),
    }
}

/// Opens `name` in `dir` for reading, never through a link, and checks it is the version `expect`.
pub(super) fn open_source(dir: &Folder, name: &OsStr, expect: Identity) -> Result<File> {
    let file = dir.open_read(name)?;
    let opened = Identity::of_file(&file).map_err(io_err("stat", &dir.join(name)))?;
    if opened != expect {
        return Err(Error::Changed(dir.join(name)));
    }
    Ok(file)
}

/// Copies exactly `size` bytes from `input` to `out` (more or fewer means the source changed while
/// it was read: [`Error::Changed`] naming `src_path`), and returns the first byte. Fault `point`:
/// an error, or a crash after half the first chunk was written.
pub(super) fn stream(input: &mut File, out: &mut File, size: u64, point: &'static str, src_path: &Path, out_path: &Path) -> Result<Option<u8>> {
    let mut buf = vec![0u8; CHUNK];
    let (mut total, mut first) = (0u64, None);
    let mut fault = faults::hit(point);
    loop {
        let n = fill(input, &mut buf).map_err(io_err("read", src_path))?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or_default();
        if first.is_none() {
            first = chunk.first().copied();
        }
        if let Some(f) = fault.take() {
            let _ = out.write_all(chunk.get(..n / 2).unwrap_or_default());
            return Err(match f {
                Fault::Error => io_err("write", out_path)(faults::injected(point)),
                Fault::Crash => Error::Crashed(point),
            });
        }
        out.write_all(chunk).map_err(io_err("write", out_path))?;
        total = total.saturating_add(n as u64);
        if total > size {
            return Err(Error::Changed(src_path.to_path_buf()));
        }
    }
    // An empty source still reaches the point.
    match fault {
        Some(Fault::Error) => return Err(io_err("write", out_path)(faults::injected(point))),
        Some(Fault::Crash) => return Err(Error::Crashed(point)),
        None => {}
    }
    if total != size {
        return Err(Error::Changed(src_path.to_path_buf()));
    }
    Ok(first)
}

/// The source's modification time, and on Unix its permission bits, onto the copy (best effort: a
/// volume that keeps neither still gets the bytes). On Windows the copy keeps its folder's
/// inherited access rules: a read-only attribute would stop the temporary name from being removed
/// when the copy gets its name.
pub(super) fn keep_attributes(out: &File, source: &Entry, modified: Option<std::time::SystemTime>) {
    if cfg!(unix) {
        let _ = Folder::copy_permissions(out, source);
    }
    if let Some(t) = modified {
        let _ = out.set_modified(t);
    }
}

/// The [`points::COPY_FLIP`] switch: armed as an error, one byte of the copy is changed on disk
/// (the first, or one appended to an empty copy), for the verification to catch.
pub(super) fn flip_switch(out: &mut File, first: Option<u8>, path: &Path) -> Result<()> {
    match faults::hit(points::COPY_FLIP) {
        Some(Fault::Error) => {
            let byte = first.map_or(0, |b| !b);
            let r = match first {
                Some(_) => out.seek(SeekFrom::Start(0)).and_then(|_| out.write_all(&[byte])),
                None => out.write_all(&[byte]),
            };
            r.and_then(|()| out.sync_all()).map_err(io_err("write", path))
        }
        Some(Fault::Crash) => Err(Error::Crashed(points::COPY_FLIP)),
        None => Ok(()),
    }
}

/// Whether `b_name` in `b` holds the same bytes as `a_name` in `a`, which must still be the version
/// `a_expect` (both opened never through a link).
pub(super) fn same_bytes(a: &Folder, a_name: &OsStr, a_expect: Identity, b: &Folder, b_name: &OsStr) -> Result<bool> {
    let mut fa = open_source(a, a_name, a_expect)?;
    let mut fb = b.open_read(b_name)?;
    let len_b = fb.metadata().map_err(io_err("stat", &b.join(b_name)))?.len();
    if len_b != a_expect.size() {
        return Ok(false);
    }
    let (mut ba, mut bb) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    loop {
        let na = fill(&mut fa, &mut ba).map_err(io_err("read", &a.join(a_name)))?;
        let nb = fill(&mut fb, &mut bb).map_err(io_err("read", &b.join(b_name)))?;
        if na != nb || ba.get(..na) != bb.get(..nb) {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

/// Reads until `buf` is full or the file ends.
fn fill(f: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while let Some(rest) = buf.get_mut(n..).filter(|r| !r.is_empty()) {
        match f.read(rest) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

const TEMP_MARK: &str = ".dalica-tmp-";
/// Longest part of the target's name kept in a temporary name, so the whole stays within 255 bytes.
const TEMP_NAME_KEEP: usize = 255 - 1 - TEMP_MARK.len() - 16;

/// A temporary name for `name`: `.<name>.dalica-tmp-<16 hex>`, the shape `atomic::is_temp_name`
/// knows (so `stale` removes a file a crash left behind).
pub(super) fn temp_name(name: &OsStr) -> OsString {
    let lossy = name.to_string_lossy();
    let mut keep = lossy.len().min(TEMP_NAME_KEEP);
    while !lossy.is_char_boundary(keep) {
        keep -= 1;
    }
    OsString::from(format!(".{}{TEMP_MARK}{}", lossy.get(..keep).unwrap_or_default(), random_hex()))
}

/// Creates a new temporary file for `name` in `dir`; returns its name and the open file.
fn create_temp(dir: &Folder, name: &OsStr) -> Result<(OsString, File)> {
    for _ in 0..16 {
        let tmp = temp_name(name);
        if let Some(f) = dir.create_new(&tmp, false)? {
            return Ok((tmp, f));
        }
    }
    Err(io_err("create", dir.path())(io::Error::new(io::ErrorKind::AlreadyExists, "no free temporary name")))
}

/// 16 hex digits from the standard library's per-process random hash keys and a counter.
fn random_hex() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u32(std::process::id());
    if let Ok(d) = UNIX_EPOCH.elapsed() {
        h.write_u128(d.as_nanos());
    }
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atomic::is_temp_name;

    #[test]
    fn temporary_names_have_the_shape_stale_removes_and_fit() {
        for name in ["photo.jpg", &"x".repeat(255), &"é".repeat(127)] {
            let t = temp_name(OsStr::new(name));
            assert!(t.len() <= 255 && is_temp_name(&t), "{t:?}");
        }
    }

    #[test]
    fn fill_reads_until_the_buffer_is_full_or_the_file_ends() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, vec![7u8; 10]).unwrap();
        let mut f = File::open(&p).unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(fill(&mut f, &mut buf).unwrap(), 4);
        assert_eq!(fill(&mut f, &mut buf).unwrap(), 4);
        assert_eq!(fill(&mut f, &mut buf).unwrap(), 2);
        assert_eq!(fill(&mut f, &mut buf).unwrap(), 0);
    }
}
