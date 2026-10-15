//! The skip rules and the File Types toggles, each on a temporary tree (`data-model.md` §7).

use std::path::Path;

use crate::common::put;
use dalica_scan::skip::{
    FOLDER_SUFFIXES, FOLDERS_ANYWHERE, MACOS_AT_HOME, MACOS_AT_ROOT, UNIX_AT_HOME, UNIX_AT_ROOT, WINDOWS_ANYWHERE, WINDOWS_AT_ROOT,
};
use dalica_scan::{FileType, FileTypes, Options, Platform, Skip, SkipRules, scan_with};
use dalica_scenes::{Scene, jpeg, png};

fn photo() -> Vec<u8> {
    jpeg(&Scene::new(1, 16, 16), 80, None).unwrap()
}

/// Every photo a scan of `root` under `rules` lists, as paths relative to `root` with `/`.
fn listed(root: &Path, rules: SkipRules, types: FileTypes) -> Vec<String> {
    let s = scan_with(&[root.to_path_buf()], &Options { skip: rules, types, ..Options::default() });
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let mut out: Vec<String> = s
        .folders
        .iter()
        .flat_map(|f| f.photos.iter().map(move |p| f.path.join(&p.name)))
        .map(|p| p.strip_prefix(root).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
        .collect();
    out.sort();
    out
}

/// The rules of `platform` over `root`, with `root/home` holding the homes.
fn rules(platform: Platform, root: &Path) -> SkipRules {
    SkipRules::for_platform(platform, root, &root.join("home"))
}

#[test]
fn dot_entries_and_the_originals_folder_are_skipped_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    put(&r.join("kept/a.jpg"), &photo());
    put(&r.join("kept/.dot.jpg"), &photo());
    put(&r.join("kept/._a.jpg"), &photo());
    put(&r.join("kept/.hidden/b.jpg"), &photo());
    put(&r.join("kept/.dalica-originals/a.jpg"), &photo());
    put(&r.join(".Trashes/c.jpg"), &photo());
    for platform in [Platform::Windows, Platform::MacOs, Platform::Unix] {
        assert_eq!(listed(r, rules(platform, r), FileTypes::all()), ["kept/a.jpg"], "{platform:?}");
    }
    let rules = rules(Platform::Unix, r);
    assert_eq!(rules.check_path(&r.join("kept/.dalica-originals"), true), Some(Skip::Originals));
    assert_eq!(rules.check_path(&r.join("kept/.DALICA-ORIGINALS"), true), Some(Skip::Originals));
    assert_eq!(rules.check_path(&r.join("kept/.dot.jpg"), false), Some(Skip::Dot));
}

#[test]
fn system_folders_and_packages_are_skipped_at_any_depth() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    put(&r.join("kept/a.jpg"), &photo());
    let mut planted = Vec::new();
    for (i, name) in FOLDERS_ANYWHERE.iter().enumerate() {
        // At the top, deep down, and in another case.
        put(&r.join(name).join("x.jpg"), &photo());
        put(&r.join(format!("deep{i}/er")).join(name.to_lowercase()).join("x.jpg"), &photo());
        planted.push(name.to_string());
    }
    for (i, suffix) in FOLDER_SUFFIXES.iter().enumerate() {
        put(&r.join(format!("pkg{i}/Old Library{suffix}/Masters/x.jpg")), &photo());
    }
    // A name that only ends like a package, and a file named like a system folder, are kept.
    put(&r.join("kept/app/b.jpg"), &photo());
    put(&r.join("kept/node_modules.jpg"), &photo());
    for platform in [Platform::Windows, Platform::MacOs, Platform::Unix] {
        assert_eq!(listed(r, rules(platform, r), FileTypes::all()), ["kept/a.jpg", "kept/app/b.jpg", "kept/node_modules.jpg"], "{platform:?}");
    }
    let rules = rules(Platform::Unix, r);
    assert_eq!(rules.check_path(&r.join("a/@eaDir"), true), Some(Skip::System));
    assert_eq!(rules.check_path(&r.join("a/Photos Library.photoslibrary"), true), Some(Skip::Package));
    assert_eq!(rules.check_path(&r.join("a/.app"), true), Some(Skip::Dot));
    assert_eq!(planted.len(), FOLDERS_ANYWHERE.len());
}

/// A system and its folder lists: skipped anywhere, at the top of the disk, at the top of a home.
type Case = (Platform, &'static [&'static str], &'static [&'static str], &'static [&'static str]);

/// The per-system folders: skipped where the rule says (anywhere, at the top of the disk, at the
/// top of a home) and kept everywhere else.
#[test]
fn each_system_skips_its_own_folders_where_they_belong() {
    let cases: [Case; 3] = [
        (Platform::Windows, &WINDOWS_ANYWHERE, &WINDOWS_AT_ROOT, &[]),
        (Platform::MacOs, &[], &MACOS_AT_ROOT, &MACOS_AT_HOME),
        (Platform::Unix, &[], &UNIX_AT_ROOT, &UNIX_AT_HOME),
    ];
    for (platform, anywhere, at_root, at_home) in cases {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        let mut kept = vec!["home/me/Pictures/a.jpg".to_string()];
        put(&r.join("home/me/Pictures/a.jpg"), &photo());
        for name in anywhere {
            put(&r.join(name).join("x.jpg"), &photo());
            put(&r.join("home/me/Pictures").join(name).join("x.jpg"), &photo());
        }
        for name in at_root {
            put(&r.join(name).join("x.jpg"), &photo());
            // The same name deeper down is the user's own folder.
            put(&r.join("home/me/Pictures").join(name).join("y.jpg"), &photo());
            kept.push(format!("home/me/Pictures/{name}/y.jpg"));
        }
        for name in at_home {
            put(&r.join("home/me").join(name).join("x.jpg"), &photo());
            put(&r.join("home/me/Pictures").join(name).join("y.jpg"), &photo());
            kept.push(format!("home/me/Pictures/{name}/y.jpg"));
        }
        kept.sort();
        kept.dedup();
        // `home` itself is no system folder, `Volumes` only on macOS.
        assert_eq!(listed(r, rules(platform, r), FileTypes::all()), kept, "{platform:?}");
        // Another system's rules keep these folders, unless that system lists the name too.
        let other = if platform == Platform::Unix { Platform::Windows } else { Platform::Unix };
        let other_lists: Vec<&str> =
            if other == Platform::Unix { UNIX_AT_ROOT.to_vec() } else { [&WINDOWS_AT_ROOT[..], &WINDOWS_ANYWHERE[..]].concat() };
        let other_rules = rules(other, r);
        let mut checked = 0;
        for name in anywhere.iter().chain(at_root).filter(|n| !other_lists.iter().any(|o| o.eq_ignore_ascii_case(n))) {
            checked += 1;
            assert_eq!(other_rules.check_path(&r.join(name), true), None, "{platform:?} {name} under {other:?}");
        }
        assert!(checked > 0, "{platform:?}");
    }
}

#[test]
fn the_system_rules_skip_volume_roots_system_folders() {
    let rules = SkipRules::system();
    let (root, kept) = if cfg!(windows) {
        (r"C:\Windows", r"C:\Users\me\Pictures\Windows")
    } else if cfg!(target_os = "macos") {
        ("/System", "/Users/me/Pictures/System")
    } else {
        ("/proc", "/home/me/Pictures/proc")
    };
    assert_eq!(rules.check_path(Path::new(root), true), Some(Skip::SystemAtRoot));
    assert_eq!(rules.check_path(Path::new(kept), true), None);
    // Files are never system folders.
    assert_eq!(rules.check_path(Path::new(root), false), None);
}

/// Bytes that sniff as `magic`'s format, padded past the sniffing length.
fn magic(bytes: &[u8]) -> Vec<u8> {
    let mut v = bytes.to_vec();
    v.resize(64, 0);
    v
}

#[test]
fn toggles_drop_exactly_their_formats() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    let files: [(&str, FileType, Vec<u8>); 10] = [
        ("a.jpg", FileType::Jpeg, photo()),
        ("b.png", FileType::Png, png(&Scene::new(2, 16, 16)).unwrap()),
        ("c.tif", FileType::Tiff, magic(b"II*\0\x08\0\0\0")),
        ("d.webp", FileType::WebP, magic(b"RIFF\x24\0\0\0WEBPVP8 ")),
        ("e.gif", FileType::Gif, magic(b"GIF89a\x01\0\x01\0")),
        ("f.bmp", FileType::Bmp, magic(b"BM\x3a\0\0\0\0\0\0\0\x36\0\0\0")),
        ("g.heic", FileType::Heif, magic(b"\0\0\0\x18ftypheic\0\0\0\0")),
        ("h.avif", FileType::Avif, magic(b"\0\0\0\x1cftypavif\0\0\0\0")),
        ("i.jxl", FileType::JpegXl, magic(b"\xff\x0a\xfa")),
        ("j.psd", FileType::Psd, magic(b"8BPS\0\x01")),
    ];
    for (name, _, bytes) in &files {
        put(&r.join("p").join(name), bytes);
    }
    // Recognised by content, whatever the name: a GIF named `.txt` is listed, text named `.jpg` is not.
    put(&r.join("p/k.txt"), &magic(b"GIF87a\x01\0\x01\0"));
    put(&r.join("p/l.jpg"), b"plain text, long enough to be sniffed as nothing at all");
    let all: Vec<String> = files.iter().map(|(n, ..)| format!("p/{n}")).chain(["p/k.txt".to_string()]).collect();
    let mut all_sorted = all.clone();
    all_sorted.sort();
    let rules = SkipRules::system();
    assert_eq!(listed(r, rules.clone(), FileTypes::all()), all_sorted);
    for ty in FileType::ALL {
        let mut want: Vec<String> = files
            .iter()
            .filter(|(_, t, _)| ty == FileType::Jpeg || *t != ty)
            .map(|(n, ..)| format!("p/{n}"))
            .chain((ty != FileType::Gif).then(|| "p/k.txt".to_string()))
            .collect();
        want.sort();
        assert_eq!(listed(r, rules.clone(), FileTypes::all().with(ty, false)), want, "{ty:?} off");
    }
    assert_eq!(listed(r, rules.clone(), FileTypes::jpeg_only()), ["p/a.jpg"]);
    // Formats not decoded yet are listed at 0 × 0 (a format tile until their decoders arrive).
    let s = scan_with(&[r.to_path_buf()], &Options::default());
    let gif = s.folders[0].photos.iter().find(|p| p.name == "e.gif").unwrap();
    assert_eq!(gif.format, dalica_image::Format::Gif);
    let a = s.folders[0].photos.iter().find(|p| p.name == "a.jpg").unwrap();
    assert_eq!((a.width, a.height), (16, 16));
}

#[test]
fn file_types_turn_on_and_off_and_have_stable_ids() {
    let t = FileTypes::all().with(FileType::Png, false).with(FileType::Raw, false);
    assert!(!t.allows(FileType::Png) && !t.allows(FileType::Raw) && t.allows(FileType::Gif));
    assert!(t.with(FileType::Png, true).allows(FileType::Png));
    assert!(FileTypes::all().with(FileType::Jpeg, false).allows(FileType::Jpeg), "JPEG is always on");
    assert!(FileTypes::jpeg_only().allows(FileType::Jpeg));
    assert!(FileType::ALL.iter().skip(1).all(|&f| !FileTypes::jpeg_only().allows(f)));
    for ty in FileType::ALL {
        assert_eq!(FileType::from_id(ty.id()), Some(ty));
    }
    assert_eq!(FileType::from_id("bogus"), None);
    assert_eq!(FileType::of(dalica_image::Format::Png), FileType::Png);
    assert!(t.allows_format(dalica_image::Format::Jpeg) && !t.allows_format(dalica_image::Format::Png));
}
