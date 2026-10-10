//! Bounded walks (`data-model.md` §7, `security.md` §5.6, §5.12): order, skip rules, limits, loops,
//! awkward names, cancellation and the UI-thread guard.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use dalica_fs::{Error, Limits, Warning, mark_ui_thread, walk};

fn touch(p: &Path) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"x").unwrap();
}

/// Paths relative to `root`, with `/` separators, in walk order.
fn rel(root: &Path, w: &dalica_fs::Walk) -> Vec<String> {
    w.entries
        .iter()
        .map(|e| e.path.strip_prefix(root).unwrap().components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        .collect()
}

#[test]
fn nested_folders_come_in_sorted_pre_order() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    for p in ["b/2.jpg", "b/1.jpg", "a/z/deep.png", "a/y.jpg", "c.jpg", "B.jpg"] {
        touch(&r.join(p));
    }
    let w = walk(r, Limits::default(), |_| false).unwrap();
    assert_eq!(rel(r, &w), ["B.jpg", "a", "a/y.jpg", "a/z", "a/z/deep.png", "b", "b/1.jpg", "b/2.jpg", "c.jpg"]);
    assert!(w.warnings.is_empty(), "{:?}", w.warnings);
    let deep = w.entries.iter().find(|e| e.path.ends_with("deep.png")).unwrap();
    assert_eq!((deep.depth, deep.is_dir, deep.size), (3, false, 1));
    assert!(w.entries.iter().find(|e| e.path.ends_with("a")).unwrap().is_dir);
}

#[test]
fn skip_rules_leave_out_entries_and_whole_folders() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    for p in [".hidden/a.jpg", ".dalica-originals/a.jpg", "keep/.dalica.json", "keep/a.jpg", "skip/a.jpg"] {
        touch(&r.join(p));
    }
    let w = walk(r, Limits::default(), |e| e.name().starts_with('.') || e.name() == "skip").unwrap();
    assert_eq!(rel(r, &w), ["keep", "keep/a.jpg"]);
}

#[test]
fn a_40_deep_tree_stops_at_depth_32_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = dir.path().to_path_buf();
    for i in 0..40 {
        p.push(format!("d{i}"));
    }
    touch(&p.join("bottom.jpg"));
    let w = walk(dir.path(), Limits::default(), |_| false).unwrap();
    assert_eq!(w.entries.len(), 32);
    assert_eq!(w.entries.iter().map(|e| e.depth).max(), Some(32));
    assert_eq!(w.warnings.len(), 1);
    assert!(matches!(&w.warnings[0], Warning::TooDeep(p) if p.ends_with("d31")), "{:?}", w.warnings);
}

#[test]
fn the_entry_limit_stops_the_walk_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..50 {
        touch(&dir.path().join(format!("f/{i:03}.jpg")));
    }
    let w = walk(dir.path(), Limits { max_entries: 10, ..Limits::default() }, |_| false).unwrap();
    assert_eq!(w.entries.len(), 10);
    assert_eq!(w.warnings, [Warning::TooManyEntries]);
    // A cut-off walk still comes back in pre-order.
    for i in 0..5 {
        touch(&dir.path().join(format!("a{i}/x.jpg")));
    }
    let w = walk(dir.path(), Limits { max_entries: 7, ..Limits::default() }, |_| false).unwrap();
    assert!(w.entries.windows(2).all(|p| p[0].path.components().lt(p[1].path.components())), "{:?}", rel(dir.path(), &w));
}

#[cfg(unix)]
#[test]
fn a_symlink_loop_ends_with_a_bounded_result_and_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    touch(&r.join("a/b/photo.jpg"));
    std::os::unix::fs::symlink(r.join("a"), r.join("a/b/up")).unwrap(); // a/b/up → a
    std::os::unix::fs::symlink(r, r.join("a/root")).unwrap(); // a/root → the root
    std::os::unix::fs::symlink(r.join("a/b/self"), r.join("a/b/self")).unwrap(); // a link to itself
    std::os::unix::fs::symlink(r.join("a/b"), r.join("other")).unwrap(); // a second way into a/b
    let w = walk(r, Limits::default(), |_| false).unwrap();
    assert_eq!(rel(r, &w), ["a", "a/b", "a/b/photo.jpg"]);
    let loops: Vec<&std::path::PathBuf> = w.warnings.iter().filter_map(|x| if let Warning::Loop(p) = x { Some(p) } else { None }).collect();
    assert_eq!(loops.len(), 3, "{:?}", w.warnings);
    // The self-link cannot be resolved: reported as unreadable, not followed.
    assert!(w.warnings.iter().any(|x| matches!(x, Warning::Unreadable { path, .. } if path.ends_with("self"))), "{:?}", w.warnings);
}

#[cfg(unix)]
#[test]
fn a_link_to_a_folder_outside_is_followed_once_and_marked() {
    let outside = tempfile::tempdir().unwrap();
    touch(&outside.path().join("x.jpg"));
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
    let w = walk(dir.path(), Limits::default(), |_| false).unwrap();
    assert_eq!(rel(dir.path(), &w), ["linked", "linked/x.jpg"]);
    assert!(w.entries[0].is_link && w.entries[0].is_dir && !w.entries[1].is_link);
}

#[test]
fn awkward_names_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    let names = ["a]b.jpg", "say \"hi\".jpg", "a;b.jpg", "a=b.jpg", "Lisboa à noite.jpg", "東京.jpg", "🌊.png", "  spaces  .jpg"];
    let names: Vec<&str> = names.into_iter().filter(|n| cfg!(unix) || !n.contains('"')).collect();
    for n in &names {
        touch(&dir.path().join(n));
    }
    let w = walk(dir.path(), Limits::default(), |_| false).unwrap();
    let mut want: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(rel(dir.path(), &w), want);
}

#[cfg(unix)]
#[test]
fn non_utf8_names_are_listed_exactly() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let name = OsStr::from_bytes(b"caf\xe9.jpg");
    if fs::write(dir.path().join(name), b"x").is_err() {
        return; // A file system that only takes UTF-8 names (e.g. some macOS volumes).
    }
    let w = walk(dir.path(), Limits::default(), |_| false).unwrap();
    assert_eq!(w.entries.len(), 1);
    assert_eq!(w.entries[0].path.file_name().unwrap(), name);
}

#[cfg(unix)]
#[test]
fn an_unreadable_folder_is_a_warning_not_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("locked/a.jpg"));
    touch(&dir.path().join("open/b.jpg"));
    let locked = dir.path().join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let readable_anyway = fs::read_dir(&locked).is_ok(); // running as root
    let w = walk(dir.path(), Limits::default(), |_| false).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    if readable_anyway {
        return;
    }
    assert_eq!(rel(dir.path(), &w), ["locked", "open", "open/b.jpg"]);
    assert!(matches!(&w.warnings[..], [Warning::Unreadable { path, .. }] if path.ends_with("locked")), "{:?}", w.warnings);
}

#[test]
fn a_set_cancel_flag_stops_the_walk() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("a/b.jpg"));
    let cancel = AtomicBool::new(true);
    let w = walk(dir.path(), Limits { cancel: Some(&cancel), ..Limits::default() }, |_| false).unwrap();
    assert!(w.entries.is_empty());
    assert_eq!(w.warnings, [Warning::Cancelled]);
}

#[test]
fn a_missing_or_file_root_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(walk(&dir.path().join("missing"), Limits::default(), |_| false), Err(Error::Io { .. })));
    touch(&dir.path().join("file.jpg"));
    assert!(matches!(walk(&dir.path().join("file.jpg"), Limits::default(), |_| false), Err(Error::Io { .. })));
}

#[test]
fn walking_on_the_ui_thread_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let _ui = mark_ui_thread();
    assert!(matches!(walk(dir.path(), Limits::default(), |_| false), Err(Error::OnUiThread)));
}
