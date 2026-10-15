//! Standard places, mounted volumes, reachability and the Folder Manager's subfolder listing.

use std::path::{Path, PathBuf};
use std::time::Instant;

use std::ffi::OsString;

use dalica_scan::volumes::{CHILDREN_CAP, PLACES_HOME_ENV, children_capped, parse_mounts, parse_user_dirs, volumes_in};
use dalica_scan::{PlaceKind, Places, VolumeKind, children, online, volumes};

#[test]
fn places_honour_the_override() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("me");
    let env = |pairs: Vec<(&'static str, OsString)>| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
    let places =
        Places::from_env(env(vec![(PLACES_HOME_ENV, home.clone().into()), ("HOME", "/elsewhere".into()), ("USERPROFILE", r"C:\elsewhere".into())]));
    assert_eq!(places, Places::under(&home));
    let all: Vec<(PlaceKind, PathBuf)> = places.all().into_iter().map(|p| (p.kind, p.path)).collect();
    assert_eq!(
        all,
        [
            (PlaceKind::Pictures, home.join("Pictures")),
            (PlaceKind::Documents, home.join("Documents")),
            (PlaceKind::Desktop, home.join("Desktop")),
            (PlaceKind::Home, home.clone()),
        ]
    );
    // Nothing exists yet; then only what was made.
    assert!(places.existing().is_empty());
    std::fs::create_dir_all(home.join("Pictures")).unwrap();
    std::fs::create_dir_all(home.join("Desktop")).unwrap();
    assert_eq!(places.existing().iter().map(|p| p.kind).collect::<Vec<_>>(), [PlaceKind::Pictures, PlaceKind::Desktop, PlaceKind::Home]);
    assert_eq!(places.first_run(), [home.join("Pictures"), home.join("Desktop")]);
    // An empty or relative override means the user's own places; no home, no places.
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let own = dir.path().join("own");
    for over in ["", "relative/folder"] {
        let got = Places::from_env(env(vec![(PLACES_HOME_ENV, over.into()), (home_var, own.clone().into())]));
        assert_eq!(got.home.as_ref(), Some(&own), "{over:?}");
    }
    assert_eq!(Places::from_env(env(vec![])), Places::default());
    assert_eq!(Places::from_env(env(vec![(home_var, "relative".into())])), Places::default());
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn the_xdg_user_dirs_file_moves_the_places() {
    let dir = tempfile::tempdir().unwrap();
    let (home, config) = (dir.path().join("me"), dir.path().join("config"));
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("user-dirs.dirs"),
        "# written by xdg-user-dirs-update\nXDG_PICTURES_DIR=\"$HOME/Bilder\"\nXDG_DESKTOP_DIR=\"/data/desk\"\n",
    )
    .unwrap();
    let vars: Vec<(&str, OsString)> = vec![("HOME", home.clone().into()), ("XDG_CONFIG_HOME", config.into())];
    let places = Places::from_env(|k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone()));
    assert_eq!(places.pictures, Some(home.join("Bilder")));
    assert_eq!(places.documents, Some(home.join("Documents")));
    assert_eq!(places.desktop, Some(PathBuf::from("/data/desk")));
}

#[test]
fn user_dirs_lines_are_read_strictly() {
    let home = Path::new("/home/me");
    let text = "XDG_PICTURES_DIR=\"$HOME/Pics\"\n  XDG_DOCUMENTS_DIR = \"$HOME/\"\nXDG_DESKTOP_DIR=\"$HOMEX/a\"\nXDG_MUSIC_DIR=\"$HOME/Music\"\nXDG_DESKTOP_DIR=relative\nXDG_DESKTOP_DIR=\"rel/a\"\n=\nbroken";
    assert_eq!(parse_user_dirs(text, home), [(PlaceKind::Pictures, home.join("Pics")), (PlaceKind::Documents, home.join(""))]);
}

#[test]
fn the_first_run_never_offers_the_home_or_a_folder_twice() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("me");
    std::fs::create_dir_all(home.join("Documents/Desktop")).unwrap();
    // Pictures unset falls back to the home on some systems; Desktop inside Documents.
    let places = Places {
        home: Some(home.clone()),
        pictures: Some(home.clone()),
        documents: Some(home.join("Documents")),
        desktop: Some(home.join("Documents/Desktop")),
    };
    assert_eq!(places.first_run(), [home.join("Documents")]);
    assert!(Places::default().first_run().is_empty() && Places::default().all().is_empty());
}

#[test]
fn a_renamed_away_root_reports_offline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("card");
    std::fs::create_dir_all(root.join("DCIM")).unwrap();
    assert!(online(&root));
    std::fs::rename(&root, dir.path().join("card-unplugged")).unwrap();
    assert!(!online(&root));
    // A file is no root either.
    std::fs::write(dir.path().join("file"), b"x").unwrap();
    assert!(!online(&dir.path().join("file")));
}

/// At full strength 10 000 folders against the default cap; in a task's gate 40 against a cap of
/// 16, the same checks.
#[test]
fn children_of_a_10_000_folder_tree_stay_within_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    let (n, cap) = if crate::common::full() { (10_000, CHILDREN_CAP) } else { (40, 16) };
    let list_capped = |p: &std::path::Path| if cap == CHILDREN_CAP { children(p) } else { children_capped(p, cap) };
    for i in 0..n {
        std::fs::create_dir(r.join(format!("f{i:05}"))).unwrap();
    }
    std::fs::create_dir(r.join(".hidden")).unwrap();
    std::fs::create_dir(r.join("@eaDir")).unwrap();
    std::fs::write(r.join("a-file.jpg"), b"x").unwrap();
    std::fs::create_dir_all(r.join("f00000/inner")).unwrap();
    let start = Instant::now();
    let list = list_capped(r).unwrap();
    println!("children of {n} folders (cap {cap}): {:?}", start.elapsed());
    assert_eq!(list.folders.len(), cap);
    assert!(list.truncated);
    assert!(list.folders.windows(2).all(|w| w[0].name < w[1].name));
    assert_eq!(list.folders[0].name, "f00000");
    assert_eq!(list.folders[0].path, r.join("f00000"));
    // A higher cap lists them all, still without the skipped ones, files or grandchildren.
    let all = children_capped(r, 2 * n).unwrap();
    assert_eq!(all.folders.len(), n);
    assert!(!all.truncated);
    assert!(all.folders.iter().all(|c| c.name.starts_with('f') && !c.is_link));
    // Exactly at the cap is not truncated.
    let exact = children_capped(r, n).unwrap();
    assert_eq!((exact.folders.len(), exact.truncated), (n, false));
    // An unreadable folder is an error, not a panic.
    assert!(children(&r.join("missing")).is_err());
}

#[cfg(unix)]
#[test]
fn children_include_links_to_folders() {
    let dir = tempfile::tempdir().unwrap();
    let (p, elsewhere) = (dir.path().join("p"), dir.path().join("elsewhere"));
    std::fs::create_dir_all(p.join("real")).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, p.join("link")).unwrap();
    // A second way into a folder already listed is listed once.
    std::os::unix::fs::symlink(p.join("real"), p.join("again")).unwrap();
    let list = children(&p).unwrap();
    assert_eq!(list.folders.iter().map(|c| (c.name.as_str(), c.is_link)).collect::<Vec<_>>(), [("link", true), ("real", false)]);
}

#[test]
fn volumes_are_the_folders_inside_mount_parents() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("Volumes");
    let b = dir.path().join("media");
    for v in ["Card", "Backup Disk"] {
        std::fs::create_dir_all(a.join(v)).unwrap();
    }
    std::fs::create_dir_all(b.join("usb")).unwrap();
    std::fs::write(a.join("not-a-volume"), b"x").unwrap();
    let list = volumes_in(&[a.clone(), dir.path().join("missing"), b.clone()]);
    let got: Vec<(&str, &Path, VolumeKind)> = list.iter().map(|v| (v.name.as_str(), v.path.as_path(), v.kind)).collect();
    assert_eq!(
        got,
        [
            ("Backup Disk", a.join("Backup Disk").as_path(), VolumeKind::Mounted),
            ("Card", a.join("Card").as_path(), VolumeKind::Mounted),
            ("usb", b.join("usb").as_path(), VolumeKind::Mounted)
        ]
    );
}

#[test]
fn a_linux_mount_table_lists_disks_and_shares() {
    let table = "\
overlay / overlay rw,relatime 0 0
proc /proc proc rw,nosuid 0 0
tmpfs /run tmpfs rw 0 0
/dev/nvme0n1p1 /boot/efi vfat rw 0 0
/dev/sdb1 /media/me/USB\\040DISK vfat rw,nosuid 0 0
/dev/sdc1 /run/media/me/card exfat rw 0 0
tmpfs /media/me/ram tmpfs rw 0 0
nas:/photos /srv/photos nfs4 rw 0 0
//nas/share /home/me/share cifs rw 0 0
/dev/sdb1 /media/me/USB\\040DISK vfat rw,nosuid 0 0
garbage
";
    let got: Vec<(String, PathBuf, VolumeKind)> = parse_mounts(table).into_iter().map(|v| (v.name, v.path, v.kind)).collect();
    assert_eq!(
        got,
        [
            ("/".to_string(), PathBuf::from("/"), VolumeKind::System),
            ("USB DISK".to_string(), PathBuf::from("/media/me/USB DISK"), VolumeKind::Mounted),
            ("card".to_string(), PathBuf::from("/run/media/me/card"), VolumeKind::Mounted),
            ("photos".to_string(), PathBuf::from("/srv/photos"), VolumeKind::Network),
            ("share".to_string(), PathBuf::from("/home/me/share"), VolumeKind::Network),
        ]
    );
    assert!(parse_mounts("").is_empty());
    // Hostile escapes are kept as written, never a panic.
    assert_eq!(parse_mounts("/dev/x /mnt/a\\9 ext4 rw 0 0\n/dev/y /mnt/b\\ ext4 rw 0 0")[0].name, "a\\9");
}

#[test]
fn this_system_has_a_startup_disk() {
    let list = volumes();
    assert!(list.iter().any(|v| v.kind == VolumeKind::System), "{list:?}");
    assert!(list.iter().all(|v| !v.name.is_empty()));
}
