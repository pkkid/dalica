//! The file holding an application's icon, for Open With ▸ (M16.23, `organizing.md` §9): the
//! system half of [`dalica_engine::os::Os::icon_file`]. The engine reads, decodes and caches what
//! it names (`dalica_engine::handler_icons`); this only finds it, off the owner thread.
//!
//! - **Linux** (a desktop entry's `Icon`): an absolute file as it is; a name looked up as a PNG in
//!   the hicolor theme under each data folder (`$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`:
//!   `icons/hicolor/<size>/apps/<name>.png`, the size nearest the one asked: the smallest at least
//!   as large, else the largest), then `/usr/share/pixmaps/<name>.png`. The user's own theme is not
//!   read (the hicolor theme is where every application installs its icon). SVG icons are none:
//!   the product has no SVG rasteriser. A link is followed here, as the engine reads no final link.
//! - **Windows** (a ProgID's `DefaultIcon`, `C:\path\app.exe,0`): an `.ico` or `.png` file as it
//!   is; an `.exe`'s or `.dll`'s icon group, by its index (`,1`: the second group) or its
//!   resource id (`,-101`), read by [`pe_icon`], a bounded reader of our own of the file's
//!   resources (no Win32 call, so the tests run it on every OS), handed over as a one-image ICO.
//! - **macOS**: none (Launch Services gives no icon string, `Handler.icon` is none there).
//!
//! Hostile files are bounded: at most [`MAX_SECTIONS`] sections, [`MAX_ENTRIES`] entries per
//! resource folder, [`MAX_GROUP`] images per group and an image of at most
//! [`dalica_engine::handler_icons::MAX_ICON_BYTES`]; anything past them, or pointing outside the
//! file, is refused with the reason, never a panic.

use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use dalica_engine::handler_icons::{IcoEntry, MAX_ICON_BYTES, ico_side, nearest};
use dalica_engine::os::IconFile;

use super::desktop::Dirs;

/// The most sections of a PE file looked at.
pub const MAX_SECTIONS: usize = 96;
/// The most entries of one resource folder looked at.
pub const MAX_ENTRIES: usize = 4096;
/// The most images of one icon group looked at.
pub const MAX_GROUP: usize = 256;
/// The most size folders of a theme looked at.
const MAX_SIZES: usize = 64;
/// The longest icon name taken, in bytes.
const MAX_NAME: usize = 255;

// --- Linux --------------------------------------------------------------------------------------

/// Linux: the PNG file of the desktop entry icon `icon` at about `px` pixels.
pub fn linux(icon: &str, px: u32, dirs: &Dirs) -> Option<IconFile> {
    let icon = icon.trim();
    if icon.is_empty() || icon.len() > 4096 || icon.contains('\0') {
        return None;
    }
    let path = Path::new(icon);
    if path.is_absolute() {
        return drawable(path).then(|| real(path)).flatten();
    }
    // A name: no folders in it. Old entries name the file with its extension.
    if icon.len() > MAX_NAME || icon.contains(['/', '\\']) || icon.starts_with('.') {
        return None;
    }
    let name = icon.strip_suffix(".png").unwrap_or(icon);
    if name.ends_with(".svg") || name.ends_with(".svgz") || name.ends_with(".xpm") {
        return None;
    }
    let file = format!("{name}.png");
    let mut found: Vec<(IcoEntry, PathBuf)> = Vec::new();
    for data in &dirs.data {
        let theme = data.join("icons").join("hicolor");
        for (side, folder) in sizes(&theme) {
            let candidate = theme.join(folder).join("apps").join(&file);
            if candidate.is_file() && !found.iter().any(|(e, _)| e.width == side) {
                found.push((IcoEntry { width: side, height: side, bits: 32 }, candidate));
            }
        }
    }
    let entries: Vec<IcoEntry> = found.iter().map(|(e, _)| *e).collect();
    if let Some((_, path)) = nearest(&entries, px).and_then(|i| found.get(i)) {
        return real(path);
    }
    let pixmap = Path::new("/usr/share/pixmaps").join(&file);
    if pixmap.is_file() { real(&pixmap) } else { None }
}

/// The size folders of a theme (`16x16`, `32x32@2`), with the side in pixels each holds.
fn sizes(theme: &Path) -> Vec<(u32, String)> {
    let Ok(listing) = std::fs::read_dir(theme) else { return Vec::new() };
    listing
        .take(MAX_SIZES * 4)
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|name| {
            let (size, scale) = name.split_once('@').unwrap_or((&name, "1"));
            let (w, h) = size.split_once('x')?;
            let (w, h, scale) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?, scale.parse::<u32>().ok()?);
            (w == h && (1..=1024).contains(&w) && (1..=4).contains(&scale)).then(|| (w * scale, name.clone()))
        })
        .take(MAX_SIZES)
        .collect()
}

/// Whether the engine can draw a file of this name: a PNG or an ICO (never an SVG or XPM).
fn drawable(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "ico")
}

/// `path` with every link followed, if it is a file.
fn real(path: &Path) -> Option<IconFile> {
    let real = std::fs::canonicalize(path).ok()?;
    real.is_file().then_some(IconFile::Path(real))
}

// --- Windows ------------------------------------------------------------------------------------

/// Windows: the icon `DefaultIcon` names (`"C:\path\app.exe",0`, `C:\icons\app.ico`) at about
/// `px` pixels.
pub fn windows(icon: &str, px: u32) -> Option<IconFile> {
    let (file, index) = split_icon(icon)?;
    let path = PathBuf::from(&file);
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "ico" | "png" => real(&path),
        "exe" | "dll" | "icl" | "cpl" | "ocx" => {
            let file = File::open(&path).ok()?;
            let source = FileSource::new(file).ok()?;
            match pe_icon(&source, index, px) {
                Ok(ico) => Some(IconFile::Bytes(ico)),
                Err(why) => {
                    log::info!("os: no icon in {}: {why}", path.display());
                    None
                }
            }
        }
        _ => None,
    }
}

/// `DefaultIcon`'s file and index: `"C:\a b\app.exe",-101` is (`C:\a b\app.exe`, -101); no
/// index is 0. `None` for an empty value or one that names the file itself (`%1`).
pub fn split_icon(icon: &str) -> Option<(String, i32)> {
    let icon = icon.trim();
    if icon.is_empty() || icon.len() > 4096 || icon.contains("%1") || icon.contains('\0') {
        return None;
    }
    let (file, index) = match icon.rsplit_once(',') {
        Some((file, index)) => match index.trim().parse::<i32>() {
            Ok(i) => (file, i),
            Err(_) => (icon, 0),
        },
        None => (icon, 0),
    };
    let file = file.trim().trim_matches('"').trim();
    (!file.is_empty()).then(|| (file.to_string(), index))
}

/// Bytes of a file read at an offset: a file on disk, or a slice (the tests' hand-built files).
pub trait ReadAt {
    /// `len` bytes at `offset`; an error past the end.
    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, String>;
}

impl ReadAt for [u8] {
    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let start = usize::try_from(offset).map_err(|_| "past the end of the file".to_string())?;
        let end = start.checked_add(len).ok_or("past the end of the file")?;
        self.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| "past the end of the file".into())
    }
}

/// A file on disk, read where it is asked (an `.exe` is never read whole).
pub struct FileSource {
    file: RefCell<File>,
    size: u64,
}

impl FileSource {
    pub fn new(file: File) -> Result<FileSource, String> {
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        Ok(FileSource { file: RefCell::new(file), size })
    }
}

impl ReadAt for FileSource {
    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        if offset.checked_add(len as u64).is_none_or(|end| end > self.size) {
            return Err("past the end of the file".into());
        }
        let mut file = self.file.try_borrow_mut().map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        buf.try_reserve_exact(len).map_err(|e| e.to_string())?;
        buf.resize(len, 0);
        file.read_exact(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).and_then(|s| s.try_into().ok()).map(u16::from_le_bytes).ok_or_else(|| "a damaged PE file".into())
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes).ok_or_else(|| "a damaged PE file".into())
}

/// The resource types read.
const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
/// The bit of a resource entry that marks a folder (in its offset) or a name (in its id).
const HIGH: u32 = 0x8000_0000;

/// One section: where its bytes are in memory (`rva`, `len`) and in the file (`raw`, `raw_len`).
struct Section {
    rva: u32,
    len: u32,
    raw: u32,
    raw_len: u32,
}

struct Pe<'a, R: ReadAt + ?Sized> {
    src: &'a R,
    sections: Vec<Section>,
    /// The resource folder's place in the file.
    root: u64,
}

impl<R: ReadAt + ?Sized> Pe<'_, R> {
    /// The file offset of `rva`, `len` bytes of which must be in the same section's file bytes.
    fn offset(&self, rva: u32, len: u32) -> Result<u64, String> {
        self.sections
            .iter()
            .find_map(|s| {
                let inside = rva.checked_sub(s.rva)?;
                let end = inside.checked_add(len)?;
                (inside < s.len.max(s.raw_len) && end <= s.raw_len).then(|| u64::from(s.raw) + u64::from(inside))
            })
            .ok_or_else(|| format!("the address {rva:#x} is outside the file's sections"))
    }

    /// A resource folder's entries at `at` (from the resource folder's start): each one's id
    /// (`None`: a name) and its target.
    fn folder(&self, at: u32) -> Result<Vec<(Option<u32>, u32)>, String> {
        let base = self.root + u64::from(at);
        let head = self.src.read_at(base, 16)?;
        let count = usize::from(u16_at(&head, 12)?) + usize::from(u16_at(&head, 14)?);
        if count > MAX_ENTRIES {
            return Err(format!("a resource folder of {count} entries"));
        }
        let raw = self.src.read_at(base + 16, count * 8)?;
        (0..count)
            .map(|i| {
                let id = u32_at(&raw, i * 8)?;
                let target = u32_at(&raw, i * 8 + 4)?;
                Ok(((id & HIGH == 0).then_some(id), target))
            })
            .collect()
    }

    /// The bytes of the first language of the resource folder `target` names.
    fn data(&self, target: u32, max: usize) -> Result<Vec<u8>, String> {
        if target & HIGH == 0 {
            return Err("a resource without a language folder".into());
        }
        let langs = self.folder(target & !HIGH)?;
        let (_, leaf) = langs.first().copied().ok_or("a resource without a language")?;
        if leaf & HIGH != 0 {
            return Err("a resource nested too deep".into());
        }
        let entry = self.src.read_at(self.root + u64::from(leaf), 8)?;
        let (rva, size) = (u32_at(&entry, 0)?, u32_at(&entry, 4)?);
        if size as usize > max {
            return Err(format!("a resource of {size} bytes"));
        }
        self.src.read_at(self.offset(rva, size)?, size as usize)
    }
}

/// The icon group `index` of the PE file `src` (`index` ≥ 0: the group at that place among the
/// file's groups; < 0: the group whose resource id is `-index`), its image nearest `px` pixels
/// put together as an ICO of that one image.
pub fn pe_icon<R: ReadAt + ?Sized>(src: &R, index: i32, px: u32) -> Result<Vec<u8>, String> {
    let dos = src.read_at(0, 64)?;
    if !dos.starts_with(b"MZ") {
        return Err("not a PE file".into());
    }
    let pe = u64::from(u32_at(&dos, 0x3c)?);
    let head = src.read_at(pe, 24)?;
    if !head.starts_with(b"PE\0\0") {
        return Err("not a PE file".into());
    }
    let count = usize::from(u16_at(&head, 6)?);
    let optional = u16_at(&head, 20)?;
    if count > MAX_SECTIONS {
        return Err(format!("{count} sections"));
    }
    let opt = src.read_at(pe + 24, usize::from(optional))?;
    let (dirs, rvas) = match u16_at(&opt, 0)? {
        0x10b => (96, 92),
        0x20b => (112, 108),
        magic => return Err(format!("an optional header of kind {magic:#x}")),
    };
    if u32_at(&opt, rvas)? < 3 {
        return Err("no resources".into());
    }
    let res_rva = u32_at(&opt, dirs + 16)?;
    let table = src.read_at(pe + 24 + u64::from(optional), count * 40)?;
    let sections = (0..count)
        .map(|i| {
            let s = i * 40;
            Ok(Section { len: u32_at(&table, s + 8)?, rva: u32_at(&table, s + 12)?, raw_len: u32_at(&table, s + 16)?, raw: u32_at(&table, s + 20)? })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut file = Pe { src, sections, root: 0 };
    file.root = file.offset(res_rva, 16)?;
    let types = file.folder(0)?;
    let of_type = |ty: u32| -> Result<Vec<(Option<u32>, u32)>, String> {
        let (_, target) = types.iter().copied().find(|(id, _)| *id == Some(ty)).ok_or("no icons")?;
        if target & HIGH == 0 {
            return Err("a damaged resource folder".into());
        }
        file.folder(target & !HIGH)
    };
    let groups = of_type(RT_GROUP_ICON)?;
    let group = if index >= 0 {
        groups.get(index as usize).map(|(_, t)| *t)
    } else {
        groups.iter().find(|(id, _)| *id == Some(index.unsigned_abs())).map(|(_, t)| *t)
    };
    let group = file.data(group.ok_or_else(|| format!("no icon group {index}"))?, 6 + 14 * MAX_GROUP)?;
    let images = usize::from(u16_at(&group, 4)?).min(MAX_GROUP);
    let mut entries = Vec::with_capacity(images);
    let mut ids = Vec::with_capacity(images);
    for i in 0..images {
        let e = group.get(6 + 14 * i..6 + 14 * (i + 1)).ok_or("a damaged icon group")?;
        let &[w, h, colours, reserved, ..] = e else { return Err("a damaged icon group".into()) };
        entries.push(IcoEntry { width: ico_side(w), height: ico_side(h), bits: u16_at(e, 6)? });
        ids.push(([w, h, colours, reserved], u16_at(e, 4)?, u16_at(e, 12)?));
    }
    let chosen = nearest(&entries, px).ok_or("an empty icon group")?;
    let (head, planes, id) = ids.get(chosen).copied().ok_or("an empty icon group")?;
    let bits = entries.get(chosen).map_or(32, |e| e.bits);
    let icons = of_type(RT_ICON)?;
    let (_, target) = icons.iter().copied().find(|(i, _)| *i == Some(u32::from(id))).ok_or_else(|| format!("no icon image {id}"))?;
    let image = file.data(target, MAX_ICON_BYTES as usize)?;
    // An ICO of the one image: its header, one entry, the image after them.
    let mut ico = Vec::with_capacity(22 + image.len());
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    ico.extend_from_slice(&head);
    ico.extend_from_slice(&planes.to_le_bytes());
    ico.extend_from_slice(&bits.to_le_bytes());
    ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(&image);
    Ok(ico)
}
