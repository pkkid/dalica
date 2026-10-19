//! What a scan leaves out (`data-model.md` §7): the skip rules for files and folders, and the
//! per-type toggles of Options → File Types.
//!
//! Every rule lives here and is listed in the crate README, so the list a user reads and the list
//! the code applies are the same. Names are compared ignoring ASCII case: the systems that put
//! these folders on a disk (Windows, macOS, NAS firmware) do not keep case either.

use std::path::{Path, PathBuf};

use dalica_fs::Entry;
use dalica_image::Format;

/// Dalica's own originals folder (`data-model.md` §6). It starts with a dot, so the dot rule covers
/// it too; it is named here so the rule survives any change to that one.
pub const ORIGINALS: &str = ".dalica-originals";

/// Folders left out wherever they are, on every system: recycle bins and volume bookkeeping
/// (Windows, Linux, NAS firmware), NAS thumbnail caches, and tool folders that never hold the
/// user's photos.
pub const FOLDERS_ANYWHERE: [&str; 13] = [
    "$RECYCLE.BIN",
    "System Volume Information",
    "lost+found",
    "@eaDir",
    "#recycle",
    "#snapshot",
    "@Recycle",
    "@Recently-Snapshot",
    "@SynoResource",
    "node_modules",
    "__pycache__",
    "$WinREAgent",
    "Config.Msi",
];

/// Folder name endings left out wherever they are: application bundles and other programs' photo
/// libraries and preview caches, whose insides are theirs (a scan would list every photo twice,
/// once more as each cached preview).
pub const FOLDER_SUFFIXES: [&str; 6] = [".app", ".photoslibrary", ".photolibrary", ".aplibrary", ".lrdata", ".cocatalogdb"];

/// Windows: cache and temporary folders left out wherever they are (`AppData` holds every
/// program's caches, browser caches included).
pub const WINDOWS_ANYWHERE: [&str; 4] = ["AppData", "Temporary Internet Files", "INetCache", "$Windows.~BT"];

/// Windows: system folders left out at the top of a drive.
pub const WINDOWS_AT_ROOT: [&str; 9] =
    ["Windows", "Program Files", "Program Files (x86)", "ProgramData", "Recovery", "PerfLogs", "MSOCache", "$Windows.~WS", "$SysReset"];

/// macOS: system folders left out at the top of the startup disk.
pub const MACOS_AT_ROOT: [&str; 14] =
    ["System", "Library", "Applications", "private", "usr", "bin", "sbin", "dev", "cores", "opt", "etc", "tmp", "var", "Volumes"];

/// macOS: folders left out at the top of a user's home (`Library` holds caches and app data).
pub const MACOS_AT_HOME: [&str; 1] = ["Library"];

/// Linux and other Unix systems: system, cache and temporary folders left out at the top of the
/// root file system.
pub const UNIX_AT_ROOT: [&str; 19] =
    ["proc", "sys", "dev", "run", "tmp", "var", "boot", "efi", "snap", "usr", "etc", "bin", "sbin", "lib", "lib32", "lib64", "libx32", "opt", "srv"];

/// Linux and other Unix systems: folders left out at the top of a user's home (`snap` holds
/// sandboxed programs' data and caches).
pub const UNIX_AT_HOME: [&str; 1] = ["snap"];

/// The systems whose folder layouts the rules know.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Windows,
    MacOs,
    /// Linux and the other Unix systems.
    Unix,
}

impl Platform {
    /// The system this build runs on.
    pub const fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Unix
        }
    }
}

/// The skip rules of one system: which files and folders a walk leaves out.
///
/// [`SkipRules::system`] is what scans use. Tests build the rules of any system over a temporary
/// tree with [`SkipRules::for_platform`], giving the folders that stand for the top of the disk and
/// the homes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkipRules {
    platform: Platform,
    /// Folders whose children are the "top of the disk" rules' targets. `None`: every volume root
    /// (a path with no grandparent, such as `/` or `C:\`).
    roots: Option<Vec<PathBuf>>,
    /// Folders holding users' homes (`/Users`, `/home`); a home is a child of one.
    homes: Vec<PathBuf>,
}

/// Why a file or folder is left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Skip {
    /// Its name starts with a dot (`.dalica.json`, `.dalica-originals`, `.git`, `.Trashes`, `._x.jpg`).
    Dot,
    /// Dalica's originals folder.
    Originals,
    /// An OS, NAS or tool folder, skipped wherever it is.
    System,
    /// Another program's bundle, library or preview cache.
    Package,
    /// A cache or temporary folder of this system.
    Cache,
    /// A system folder at the top of a disk or a home.
    SystemAtRoot,
}

impl SkipRules {
    /// The rules of the system this build runs on.
    pub fn system() -> SkipRules {
        let platform = Platform::current();
        match platform {
            Platform::Windows => SkipRules { platform, roots: None, homes: Vec::new() },
            Platform::MacOs => SkipRules { platform, roots: Some(vec![PathBuf::from("/")]), homes: vec![PathBuf::from("/Users")] },
            Platform::Unix => SkipRules { platform, roots: Some(vec![PathBuf::from("/")]), homes: vec![PathBuf::from("/home")] },
        }
    }

    /// The rules of `platform`, with `root` standing for the top of the disk and `homes` for the
    /// folder that holds users' homes.
    pub fn for_platform(platform: Platform, root: &Path, homes: &Path) -> SkipRules {
        SkipRules { platform, roots: Some(vec![root.to_path_buf()]), homes: vec![homes.to_path_buf()] }
    }

    /// Why `entry` is left out (and, for a folder, everything in it), or `None` if it is kept.
    pub fn check(&self, entry: &Entry) -> Option<Skip> {
        self.check_path(&entry.path, entry.is_dir)
    }

    /// Why the file or folder at `path` is left out, or `None` if it is kept. Looks only at the
    /// path, never at the disk.
    pub fn check_path(&self, path: &Path, is_dir: bool) -> Option<Skip> {
        let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        if name.eq_ignore_ascii_case(ORIGINALS) {
            return Some(Skip::Originals);
        }
        if name.starts_with('.') {
            return Some(Skip::Dot);
        }
        if !is_dir {
            return None;
        }
        let is = |list: &[&str]| list.iter().any(|s| s.eq_ignore_ascii_case(&name));
        if is(&FOLDERS_ANYWHERE) {
            return Some(Skip::System);
        }
        if FOLDER_SUFFIXES.iter().any(|s| ends_with_ignore_case(&name, s)) {
            return Some(Skip::Package);
        }
        let parent = path.parent();
        let at_root = parent.is_some_and(|p| match &self.roots {
            Some(roots) => roots.iter().any(|r| r == p),
            None => p.parent().is_none(),
        });
        let at_home = parent.and_then(Path::parent).is_some_and(|g| self.homes.iter().any(|h| h == g));
        let (anywhere, root, home): (&[&str], &[&str], &[&str]) = match self.platform {
            Platform::Windows => (&WINDOWS_ANYWHERE, &WINDOWS_AT_ROOT, &[]),
            Platform::MacOs => (&[], &MACOS_AT_ROOT, &MACOS_AT_HOME),
            Platform::Unix => (&[], &UNIX_AT_ROOT, &UNIX_AT_HOME),
        };
        if is(anywhere) {
            return Some(Skip::Cache);
        }
        if (at_root && is(root)) || (at_home && is(home)) {
            return Some(Skip::SystemAtRoot);
        }
        None
    }

    /// Whether `entry` is left out.
    pub fn skips(&self, entry: &Entry) -> bool {
        self.check(entry).is_some()
    }
}

fn ends_with_ignore_case(name: &str, suffix: &str) -> bool {
    name.len() > suffix.len() && name.get(name.len() - suffix.len()..).is_some_and(|end| end.eq_ignore_ascii_case(suffix))
}

/// Whether a walk leaves `entry` out (and, for a folder, everything in it) under this system's
/// rules ([`SkipRules::system`]).
pub fn skipped(entry: &Entry) -> bool {
    SkipRules::system().skips(entry)
}

/// The kinds of file Options → File Types turns on and off ("Show JPEG files and: …").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileType {
    /// Always on.
    Jpeg,
    Png,
    Gif,
    Bmp,
    Tiff,
    WebP,
    Heif,
    Avif,
    JpegXl,
    Psd,
    /// Every RAW container.
    Raw,
    /// MP4, MOV, M4V and MKV.
    Video,
}

impl FileType {
    /// Every type, in the order Options → File Types lists them.
    pub const ALL: [FileType; 12] = [
        FileType::Jpeg,
        FileType::Png,
        FileType::Gif,
        FileType::Bmp,
        FileType::Tiff,
        FileType::WebP,
        FileType::Heif,
        FileType::Avif,
        FileType::JpegXl,
        FileType::Psd,
        FileType::Raw,
        FileType::Video,
    ];

    /// The type a format recognised by its content belongs to.
    pub fn of(format: Format) -> FileType {
        match format {
            Format::Jpeg => FileType::Jpeg,
            Format::Png => FileType::Png,
            Format::Tiff => FileType::Tiff,
            Format::WebP => FileType::WebP,
            Format::Gif => FileType::Gif,
            Format::Bmp => FileType::Bmp,
            Format::Heif => FileType::Heif,
            Format::Avif => FileType::Avif,
            Format::JpegXl => FileType::JpegXl,
            Format::Psd => FileType::Psd,
            // Formats `dalica-image` learns later are the RAW containers it tells apart by content
            // (`roadmap.md` M5.1); they fall under the RAW toggle without a change here.
            #[allow(unreachable_patterns)]
            _ => FileType::Raw,
        }
    }

    /// The stable id settings and the control API use (`"jpeg"`, `"png"`, …, `"raw"`, `"video"`).
    pub const fn id(self) -> &'static str {
        match self {
            FileType::Jpeg => "jpeg",
            FileType::Png => "png",
            FileType::Gif => "gif",
            FileType::Bmp => "bmp",
            FileType::Tiff => "tiff",
            FileType::WebP => "webp",
            FileType::Heif => "heif",
            FileType::Avif => "avif",
            FileType::JpegXl => "jxl",
            FileType::Psd => "psd",
            FileType::Raw => "raw",
            FileType::Video => "video",
        }
    }

    /// The type with this [`FileType::id`].
    pub fn from_id(id: &str) -> Option<FileType> {
        FileType::ALL.into_iter().find(|t| t.id() == id)
    }
}

/// Which types a scan lists: every one by default. JPEG cannot be turned off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileTypes {
    /// One bit per [`FileType`] that is off, by its index in [`FileType::ALL`].
    off: u16,
}

impl Default for FileTypes {
    fn default() -> Self {
        FileTypes::all()
    }
}

impl FileTypes {
    /// Every type on.
    pub const fn all() -> FileTypes {
        FileTypes { off: 0 }
    }

    /// JPEG only.
    pub fn jpeg_only() -> FileTypes {
        FileType::ALL.into_iter().fold(FileTypes::all(), |t, f| t.with(f, false))
    }

    /// These types with `ty` turned on or off. Turning JPEG off changes nothing.
    #[must_use]
    pub fn with(self, ty: FileType, on: bool) -> FileTypes {
        if ty == FileType::Jpeg {
            return self;
        }
        let bit = 1u16 << (ty as u16);
        FileTypes { off: if on { self.off & !bit } else { self.off | bit } }
    }

    /// Whether a scan lists files of `ty`.
    pub const fn allows(self, ty: FileType) -> bool {
        self.off & (1u16 << (ty as u16)) == 0
    }

    /// Whether a scan lists files of `format`.
    pub fn allows_format(self, format: Format) -> bool {
        // Videos are not listed yet, as before M15.2 sniffed them; M15.11 replaces this with fileTypes.video.
        !matches!(format, Format::Video(_)) && self.allows(FileType::of(format))
    }
}
