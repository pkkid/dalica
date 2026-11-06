//! The applications' icons of Open With ▸ (M16.23, `organizing.md` §9, `menus-and-shortcuts.md`
//! §4): [`crate::os::Handler::icon`] (a theme icon's name, a file, `file.exe,index`) turned into a
//! small picture the window draws left of the application's name.
//!
//! - **Found by the system half**: [`crate::os::Os::icon_file`] says which file holds the icon at
//!   about the size asked (or hands over its bytes, an `.exe`'s icon group as an ICO).
//! - **Read and decoded here, off the owner thread**: one helper thread at a time works through the
//!   icons asked for; the file is read whole under [`MAX_ICON_BYTES`] (`dalica-fs`, a final link
//!   refused) and decoded under small limits ([`limits`]): a PNG through `dalica-image`, an ICO's
//!   entry nearest the size (a PNG through `dalica-image`, or its BMP image read by
//!   [`decode_dib`], bounded by its 256-pixel sides). The picture is scaled to fit the size asked,
//!   its alpha premultiplied while it is averaged so transparent edges keep no dark fringe.
//! - **Cached by icon string and size for the session**: a file is never read twice; a missing,
//!   damaged or oversized icon is remembered as none ([`HandlerIcon::None`]), never an error and
//!   never a panic. At most [`MAX_CACHED`] icons are kept; past that, new ones are none.
//!
//! The window asks by the handler's id ([`Session::handler_icon`]): `photo.openWithList` and
//! `viewer.fileTypes` remember which icon each handler they listed has. The cache lives beside the
//! session's [`Os`] ([`with_icons`], which [`Session::set_os`] wraps every service in), so a newly
//! plugged service starts with an empty one.
//!
//! SVG-only icons are none: the product has no SVG rasteriser (the alternative, `resvg`, would go
//! through `cargo xtask deps`).

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dalica_image::{DecodeOptions, Format, Limits, Rgba8};
use dalica_scan::FileType;

use crate::os::{Clip, DefaultSet, Handler, IconFile, Os, OsError, Service};
use crate::session::Session;

/// The largest icon file read, in bytes: anything larger is none.
pub const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;
/// The largest side of an icon decoded, in pixels.
pub const MAX_ICON_SIDE: u32 = 1024;
/// The largest size an icon is asked at, in pixels (a 16 pt row at 16× scale).
pub const MAX_ICON_PX: u32 = 256;
/// The most icons one session keeps.
pub const MAX_CACHED: usize = 512;
/// The most handlers whose icon one session remembers.
pub const MAX_KNOWN: usize = 4096;
/// The most entries of an ICO looked at.
pub const MAX_ICO_ENTRIES: usize = 256;

/// A handler's icon as the window asks for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandlerIcon {
    /// No icon to draw: the system gives none, or it is missing, damaged, too large or SVG.
    None,
    /// Being read on a helper thread: ask again in a later frame.
    Loading,
    /// The picture, fitted to the size asked.
    Ready(Arc<Rgba8>),
}

/// The limits an icon is decoded under: small pictures, small allocations.
pub fn limits() -> Limits {
    Limits {
        max_side: MAX_ICON_SIDE,
        max_pixels: u64::from(MAX_ICON_SIDE) * u64::from(MAX_ICON_SIDE),
        max_alloc: 64 * 1024 * 1024,
        max_thumbnail: MAX_ICON_BYTES,
        max_segments: 1024,
        max_header_bytes: MAX_ICON_BYTES,
        max_icc: 1024 * 1024,
        ..Limits::default()
    }
}

// --- the cache ----------------------------------------------------------------------------------

enum Slot {
    Loading,
    Done(Option<Arc<Rgba8>>),
}

#[derive(Default)]
struct Inner {
    /// Each handler's icon string, by the handler's id.
    icons_of: HashMap<String, String>,
    /// Each icon by its string and size.
    slots: HashMap<(String, u32), Slot>,
    /// The icons still to read, oldest first.
    queue: VecDeque<(String, u32)>,
    /// Whether a helper thread is working through the queue.
    working: bool,
}

/// The icons of one session's [`Os`].
#[derive(Default)]
pub struct HandlerIcons {
    inner: Mutex<Inner>,
}

impl HandlerIcons {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Remembers the icon of each of `handlers` that has one.
    pub fn remember(&self, handlers: &[Handler]) {
        let mut inner = self.lock();
        for h in handlers {
            let Some(icon) = h.icon.as_deref().filter(|i| !i.trim().is_empty()) else { continue };
            if inner.icons_of.len() >= MAX_KNOWN && !inner.icons_of.contains_key(&h.id) {
                continue;
            }
            inner.icons_of.insert(h.id.clone(), icon.to_string());
        }
    }

    /// The icon string of the handler `id`, if a listing gave one.
    pub fn icon_of(&self, id: &str) -> Option<String> {
        self.lock().icons_of.get(id).cloned()
    }

    /// The picture of `icon` at `px` pixels: ready, still loading (a helper thread reads it), or
    /// none. Never blocks and never touches a file on the calling thread.
    pub fn get(self: &Arc<Self>, os: &Arc<dyn Os>, icon: &str, px: u32) -> HandlerIcon {
        if icon.trim().is_empty() || px == 0 {
            return HandlerIcon::None;
        }
        let px = px.min(MAX_ICON_PX);
        let key = (icon.to_string(), px);
        let mut inner = self.lock();
        match inner.slots.get(&key) {
            Some(Slot::Done(Some(p))) => return HandlerIcon::Ready(Arc::clone(p)),
            Some(Slot::Done(None)) => return HandlerIcon::None,
            Some(Slot::Loading) => return HandlerIcon::Loading,
            None if inner.slots.len() >= MAX_CACHED => return HandlerIcon::None,
            None => {}
        }
        inner.slots.insert(key.clone(), Slot::Loading);
        inner.queue.push_back(key);
        if !inner.working {
            let (icons, os) = (Arc::clone(self), Arc::clone(os));
            let spawned = std::thread::Builder::new().name("dalica-icons".into()).spawn(move || work(&icons, os.as_ref()));
            match spawned {
                Ok(_) => inner.working = true,
                Err(e) => {
                    log::warn!("icons: no thread to read them: {e}");
                    let queued: Vec<_> = inner.queue.drain(..).collect();
                    for k in queued {
                        inner.slots.insert(k, Slot::Done(None));
                    }
                    return HandlerIcon::None;
                }
            }
        }
        HandlerIcon::Loading
    }
}

/// The helper thread: reads the queued icons one by one until none is left.
fn work(icons: &HandlerIcons, os: &dyn Os) {
    loop {
        let next = {
            let mut inner = icons.lock();
            match inner.queue.pop_front() {
                Some(k) => k,
                None => {
                    inner.working = false;
                    return;
                }
            }
        };
        let picture = catch_unwind(AssertUnwindSafe(|| load(os, &next.0, next.1))).unwrap_or_else(|_| {
            log::warn!("icons: reading {:?} panicked", next.0);
            None
        });
        icons.lock().slots.insert(next, Slot::Done(picture.map(Arc::new)));
    }
}

/// Reads and decodes `icon` at `px` pixels, on the calling thread: `None` when the system has no
/// file for it or the file is missing, too large or does not decode.
pub fn load(os: &dyn Os, icon: &str, px: u32) -> Option<Rgba8> {
    let bytes = match os.icon_file(icon, px)? {
        IconFile::Path(path) => read(&path)?,
        IconFile::Bytes(bytes) if bytes.len() as u64 <= MAX_ICON_BYTES => bytes,
        IconFile::Bytes(bytes) => {
            log::info!("icons: {icon:?} is {} bytes, more than {MAX_ICON_BYTES}", bytes.len());
            return None;
        }
    };
    decode(&bytes, px).map_err(|e| log::info!("icons: {icon:?} does not decode: {e}")).ok()
}

fn read(path: &Path) -> Option<Vec<u8>> {
    match dalica_fs::read(path, MAX_ICON_BYTES) {
        Ok(Some(r)) => Some(r.bytes),
        Ok(None) => None,
        Err(e) => {
            log::info!("icons: {e}");
            None
        }
    }
}

// --- decoding -----------------------------------------------------------------------------------

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Decodes an icon file's bytes (a PNG, or an ICO's entry nearest `px`) fitted to `px` pixels on
/// its longer side.
pub fn decode(bytes: &[u8], px: u32) -> Result<Rgba8, String> {
    let px = px.clamp(1, MAX_ICON_PX);
    let picture = if bytes.starts_with(PNG_SIGNATURE) {
        png(bytes)?
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        ico(bytes, px)?
    } else {
        return Err("not a PNG or an ICO".into());
    };
    fit(&picture, px)
}

fn png(bytes: &[u8]) -> Result<Rgba8, String> {
    dalica_image::decode_as(Format::Png, bytes, &DecodeOptions::default(), &limits()).map_err(|e| e.to_string())
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// One image an ICO (or an icon group) lists: its sides (0 stands for 256) and bit depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IcoEntry {
    pub width: u32,
    pub height: u32,
    pub bits: u16,
}

/// Which of `entries` to draw at `px` pixels: the smallest at least that large (the deepest
/// colour among equals), else the largest. `None` for no entries.
pub fn nearest(entries: &[IcoEntry], px: u32) -> Option<usize> {
    let side = |e: &IcoEntry| e.width.max(e.height);
    let rank = |e: &IcoEntry| (side(e) < px, if side(e) >= px { side(e) } else { u32::MAX - side(e) }, u16::MAX - e.bits);
    entries.iter().enumerate().min_by_key(|(_, e)| rank(e)).map(|(i, _)| i)
}

/// The side an ICO's byte says (0 is 256).
pub fn ico_side(b: u8) -> u32 {
    if b == 0 { 256 } else { u32::from(b) }
}

fn ico(bytes: &[u8], px: u32) -> Result<Rgba8, String> {
    let bad = || "a damaged ICO".to_string();
    let count = usize::from(u16_at(bytes, 4).ok_or_else(bad)?).min(MAX_ICO_ENTRIES);
    let mut entries = Vec::with_capacity(count);
    let mut spans = Vec::with_capacity(count);
    for i in 0..count {
        let at = 6 + 16 * i;
        let e = bytes.get(at..at + 16).ok_or_else(bad)?;
        let &[w, h, ..] = e else { return Err(bad()) };
        let (w, h) = (ico_side(w), ico_side(h));
        let bits = u16_at(e, 6).ok_or_else(bad)?;
        let (size, offset) = (u32_at(e, 8).ok_or_else(bad)?, u32_at(e, 12).ok_or_else(bad)?);
        entries.push(IcoEntry { width: w, height: h, bits });
        spans.push((offset as usize, size as usize));
    }
    let i = nearest(&entries, px).ok_or_else(|| "an ICO without images".to_string())?;
    let (offset, size) = spans.get(i).copied().ok_or_else(bad)?;
    let data = bytes.get(offset..offset.checked_add(size).ok_or_else(bad)?).ok_or_else(bad)?;
    if data.starts_with(PNG_SIGNATURE) { png(data) } else { decode_dib(data) }
}

/// An ICO's BMP image (a `BITMAPINFOHEADER` without the file header, its height doubled for the
/// AND mask under it): 32-bit with alpha (the mask when every alpha is 0), 24-bit, or 8, 4 and
/// 1-bit with a palette, the mask's set bits transparent. Uncompressed only; at most 256 × 256.
pub fn decode_dib(data: &[u8]) -> Result<Rgba8, String> {
    let bad = || "a damaged icon image".to_string();
    let header = u32_at(data, 0).ok_or_else(bad)? as usize;
    if !(40..=1024).contains(&header) {
        return Err(bad());
    }
    let width = i32::from_le_bytes(data.get(4..8).ok_or_else(bad)?.try_into().map_err(|_| bad())?);
    let height2 = i32::from_le_bytes(data.get(8..12).ok_or_else(bad)?.try_into().map_err(|_| bad())?);
    let bits = u16_at(data, 14).ok_or_else(bad)?;
    let compression = u32_at(data, 16).ok_or_else(bad)?;
    let used = u32_at(data, 32).ok_or_else(bad)?;
    let w = usize::try_from(width).ok().filter(|w| (1..=256).contains(w)).ok_or_else(bad)?;
    let h = usize::try_from(height2.unsigned_abs() / 2).ok().filter(|h| (1..=256).contains(h)).ok_or_else(bad)?;
    let bottom_up = height2 > 0;
    if compression != 0 && !(compression == 3 && bits == 32) {
        return Err(format!("a compressed icon image ({compression})"));
    }
    let palette_len = match bits {
        1 | 4 | 8 => {
            let max = 1usize << bits;
            if used == 0 { max } else { (used as usize).min(max) }
        }
        24 | 32 => 0,
        _ => return Err(format!("a {bits}-bit icon image")),
    };
    let palette_at = header + if compression == 3 { 12 } else { 0 };
    let palette = data.get(palette_at..palette_at + 4 * palette_len).ok_or_else(bad)?;
    let pixels_at = palette_at + 4 * palette_len;
    let stride = (w * usize::from(bits)).div_ceil(32) * 4;
    let mask_stride = w.div_ceil(32) * 4;
    let pixels = data.get(pixels_at..pixels_at + stride * h).ok_or_else(bad)?;
    // The mask may be missing in a 32-bit image; then nothing is masked.
    let mask = data.get(pixels_at + stride * h..pixels_at + stride * h + mask_stride * h);
    let mut out = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    for y in 0..h {
        let src = if bottom_up { h - 1 - y } else { y };
        let row = pixels.get(src * stride..(src + 1) * stride).ok_or_else(bad)?;
        for x in 0..w {
            let rgba = match bits {
                32 => {
                    let &[b, g, r, a] = row.get(x * 4..x * 4 + 4).ok_or_else(bad)? else { return Err(bad()) };
                    [r, g, b, a]
                }
                24 => {
                    let &[b, g, r] = row.get(x * 3..x * 3 + 3).ok_or_else(bad)? else { return Err(bad()) };
                    [r, g, b, 255]
                }
                _ => {
                    let per = 8 / usize::from(bits);
                    let byte = *row.get(x / per).ok_or_else(bad)?;
                    let shift = 8 - usize::from(bits) * (x % per + 1);
                    let index = usize::from(byte >> shift) & ((1 << bits) - 1);
                    match palette.get(index * 4..index * 4 + 4) {
                        Some(&[b, g, r, _]) => [r, g, b, 255],
                        _ => [0, 0, 0, 0],
                    }
                }
            };
            any_alpha |= bits == 32 && rgba[3] != 0;
            let at = (y * w + x) * 4;
            if let Some(slot) = out.get_mut(at..at + 4) {
                slot.copy_from_slice(&rgba);
            }
        }
    }
    // The AND mask: a set bit is transparent. A 32-bit image with its own alpha ignores it.
    if !any_alpha && let Some(mask) = mask {
        for y in 0..h {
            let src = if bottom_up { h - 1 - y } else { y };
            for x in 0..w {
                let set = mask.get(src * mask_stride + x / 8).is_some_and(|b| b & (0x80 >> (x % 8)) != 0);
                if let Some(a) = out.get_mut((y * w + x) * 4 + 3) {
                    *a = if set { 0 } else { 255 };
                }
            }
        }
    }
    Ok(Rgba8 { width: w as u32, height: h as u32, data: out })
}

/// `picture` fitted to `px` pixels on its longer side (never enlarged), averaged with its alpha
/// premultiplied so transparent pixels lend no colour to the edges.
pub fn fit(picture: &Rgba8, px: u32) -> Result<Rgba8, String> {
    if picture.width.max(picture.height) <= px {
        return Ok(picture.clone());
    }
    let mut pre = picture.clone();
    for [r, g, b, a] in pre.data.as_chunks_mut::<4>().0 {
        let a = u32::from(*a);
        for c in [r, g, b] {
            *c = ((u32::from(*c) * a + 127) / 255) as u8;
        }
    }
    let mut out = dalica_image::resize::fit(&pre, px).map_err(|e| e.to_string())?;
    for [r, g, b, a] in out.data.as_chunks_mut::<4>().0 {
        if *a > 0 {
            let a = u32::from(*a);
            for c in [r, g, b] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(out)
}

// --- beside the session's Os --------------------------------------------------------------------

/// The session's [`Os`] with its icon cache: every call goes to the service plugged in.
struct WithIcons {
    os: Arc<dyn Os>,
    icons: Arc<HandlerIcons>,
}

/// `os` with an empty icon cache beside it (a service that already has one gets a fresh one).
pub fn with_icons(os: Arc<dyn Os>) -> Arc<dyn Os> {
    let any: Arc<dyn Any + Send + Sync> = os.clone();
    let os = match any.downcast::<WithIcons>() {
        Ok(w) => Arc::clone(&w.os),
        Err(_) => os,
    };
    Arc::new(WithIcons { os, icons: Arc::default() })
}

impl Os for WithIcons {
    fn available(&self, service: Service) -> Result<(), OsError> {
        self.os.available(service)
    }

    fn reveal(&self, path: &Path) -> Result<(), OsError> {
        self.os.reveal(path)
    }

    fn handlers(&self, path: &Path) -> Result<Vec<Handler>, OsError> {
        self.os.handlers(path)
    }

    fn open_with(&self, path: &Path, handler: &str) -> Result<(), OsError> {
        self.os.open_with(path, handler)
    }

    fn open_in_editor(&self, paths: &[std::path::PathBuf], editor: &str) -> Result<(), OsError> {
        self.os.open_in_editor(paths, editor)
    }

    fn put_on_clipboard(&self, clip: &Clip) -> Result<(), OsError> {
        self.os.put_on_clipboard(clip)
    }

    fn default_handler(&self, ty: FileType) -> Result<Option<Handler>, OsError> {
        self.os.default_handler(ty)
    }

    fn set_default_viewer(&self, types: &[FileType]) -> Result<DefaultSet, OsError> {
        self.os.set_default_viewer(types)
    }

    fn icon_file(&self, icon: &str, px: u32) -> Option<IconFile> {
        self.os.icon_file(icon, px)
    }
}

impl Session {
    /// The session's icon cache and the service it reads through, a fresh one beside the service
    /// in use when it has none yet (a session that was never given one by [`Session::set_os`]).
    fn icons(&mut self) -> (Arc<dyn Os>, Arc<HandlerIcons>) {
        let any: Arc<dyn Any + Send + Sync> = self.os.clone();
        if let Ok(w) = any.downcast::<WithIcons>() {
            return (Arc::clone(&w.os), Arc::clone(&w.icons));
        }
        let os = Arc::clone(&self.os);
        self.os = with_icons(os);
        self.icons()
    }

    /// Remembers the icons of `handlers` (a listing's), for [`Session::handler_icon`].
    pub(crate) fn remember_icons(&mut self, handlers: &[Handler]) {
        self.icons().1.remember(handlers);
    }

    /// The icon of the handler `id` (one `photo.openWithList` or `viewer.fileTypes` listed) at
    /// `px` pixels: ready, loading on a helper thread (ask again in a later frame), or none. Never
    /// blocks; the owner thread makes no file call.
    pub fn handler_icon(&mut self, id: &str, px: u32) -> HandlerIcon {
        let (os, icons) = self.icons();
        match icons.icon_of(id) {
            Some(icon) => icons.get(&os, &icon, px),
            None => HandlerIcon::None,
        }
    }

    /// The picture of the icon string `icon` (as [`Handler::icon`] gives it) at `px` pixels, as
    /// [`Session::handler_icon`].
    pub fn icon_picture(&mut self, icon: &str, px: u32) -> HandlerIcon {
        let (os, icons) = self.icons();
        icons.get(&os, icon, px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(side: u32, bits: u16) -> IcoEntry {
        IcoEntry { width: side, height: side, bits }
    }

    #[test]
    fn the_nearest_entry_is_the_smallest_large_enough_else_the_largest() {
        let entries = [entry(16, 8), entry(32, 32), entry(32, 8), entry(48, 32)];
        assert_eq!(nearest(&entries, 16), Some(0));
        assert_eq!(nearest(&entries, 20), Some(1), "32 px, the deeper colour");
        assert_eq!(nearest(&entries, 64), Some(3), "none large enough: the largest");
        assert_eq!(nearest(&[], 16), None);
    }

    #[test]
    fn a_dib_reads_its_palette_and_its_mask() {
        // A 2 × 2 1-bit image: palette black and red, bottom-up; the mask hides the bottom left.
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&2i32.to_le_bytes());
        dib.extend_from_slice(&4i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&[0; 24]);
        dib.extend_from_slice(&[0, 0, 0, 0, 0, 0, 255, 0]);
        // XOR rows (bottom first): bottom row 1,0; top row 0,1.
        dib.extend_from_slice(&[0b1000_0000, 0, 0, 0, 0b0100_0000, 0, 0, 0]);
        // AND rows: bottom row hides x = 0.
        dib.extend_from_slice(&[0b1000_0000, 0, 0, 0, 0, 0, 0, 0]);
        let p = decode_dib(&dib).unwrap();
        assert_eq!((p.width, p.height), (2, 2));
        assert_eq!(&p.data[..8], &[0, 0, 0, 255, 255, 0, 0, 255], "the top row");
        assert_eq!(&p.data[8..], &[255, 0, 0, 0, 0, 0, 0, 255], "the bottom row, its left pixel masked");
        assert!(decode_dib(&dib[..50]).is_err(), "cut");
    }
}
