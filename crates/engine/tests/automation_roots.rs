//! Automation's path policy (`roadmap.md` M10.16; `control-protocol.md` §5.6–§5.7, `mcp.md` §5):
//! what is allowed and what is refused, with the code each refusal carries. Paths are built with
//! `Path::join`; no name Windows forbids is made on Windows.

use std::path::{Path, PathBuf};

use dalica_engine::automation::{self, ORIGINALS, Output, Policy, Refusal, Refused};
use dalica_engine::registry::Effect;

fn code<T>(r: Result<T, Refused>) -> &'static str {
    match r {
        Ok(_) => "ok",
        Err(e) => e.code(),
    }
}

/// A scratch tree: `photos` (watched, with `.dalica-originals` and `taken.jpg`), `extra` (a read
/// root), `out` (a write root, with `taken.png`), `other` (outside everything). Resolved and in the
/// plain form the policy answers in, so expected paths compare equal on systems whose temporary
/// folder is a link (macOS) or resolves to a verbatim `\\?\C:\…` path (Windows), and are paths an
/// agent may give (`automation::lexical` refuses a verbatim one).
struct Tree {
    _dir: tempfile::TempDir,
    photos: PathBuf,
    extra: PathBuf,
    out: PathBuf,
    other: PathBuf,
}

fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let root = automation::plain(&std::fs::canonicalize(dir.path()).unwrap());
    let t = Tree { photos: root.join("photos"), extra: root.join("extra"), out: root.join("out"), other: root.join("other"), _dir: dir };
    for d in [&t.photos, &t.extra, &t.out, &t.other, &t.photos.join(ORIGINALS), &t.extra.join("2026-07 More")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(t.photos.join("taken.jpg"), b"x").unwrap();
    std::fs::write(t.out.join("taken.png"), b"x").unwrap();
    t
}

fn policy(t: &Tree) -> Policy {
    Policy::with_roots(std::slice::from_ref(&t.extra), std::slice::from_ref(&t.out))
}

#[test]
fn reads_stay_inside_a_read_root_or_a_watched_folder() {
    let t = tree();
    let p = policy(&t);
    let watched = std::slice::from_ref(&t.photos);
    assert_eq!(p.check_read(&t.extra.join("2026-07 More"), "path", watched).unwrap(), t.extra.join("2026-07 More"));
    assert_eq!(code(p.check_read(&t.photos, "path", watched)), "ok", "a watched folder may be added again");
    assert_eq!(code(p.check_read(&t.other, "path", watched)), "path_denied");
    // `..` is refused before anything is resolved, even when it would land inside a root.
    assert_eq!(code(p.check_read(&t.extra.join("..").join("extra"), "path", watched)), "path_denied");
    assert_eq!(code(p.check_read(&t.extra.join("missing"), "path", watched)), "not_found");
    assert_eq!(code(p.check_read(Path::new("relative"), "path", watched)), "bad_params");
    // No read root at all: refused, saying how to grant one; the demo may add anything.
    let none = Policy::default();
    let e = none.check_read(&t.other, "path", &[]).unwrap_err();
    assert!(e.kind == Refusal::PathDenied && e.message.contains("--automation-read-root"), "{e}");
    let mut demo = Policy::default();
    demo.set_demo(true);
    assert_eq!(code(demo.check_read(&t.other, "path", &[])), "ok");
    #[cfg(unix)]
    {
        // A link inside a read root that points out of it is followed before the check.
        std::os::unix::fs::symlink(&t.other, t.extra.join("link")).unwrap();
        assert_eq!(code(p.check_read(&t.extra.join("link"), "path", watched)), "path_denied");
    }
}

#[test]
fn new_files_go_into_a_write_root_never_into_originals_a_watched_folder_or_over_a_file() {
    let t = tree();
    let p = policy(&t);
    let watched = std::slice::from_ref(&t.photos);
    // Screenshots and renders: a write root only; a `.png` there may be replaced.
    assert_eq!(p.check_write(&t.out.join("shot.png"), "path", watched).unwrap(), t.out.join("shot.png"));
    assert_eq!(code(p.check_write(&t.out.join("taken.png"), "path", watched)), "ok");
    assert_eq!(code(p.check_write(&t.photos.join("shot.png"), "path", watched)), "path_denied", "a watched folder");
    assert_eq!(code(p.check_write(&t.photos.join(ORIGINALS).join("shot.png"), "path", watched)), "path_denied");
    assert_eq!(code(p.check_write(&t.other.join("shot.png"), "path", watched)), "path_denied");
    assert_eq!(code(p.check_write(&t.out.join("missing").join("shot.png"), "path", watched)), "not_found");
    assert_eq!(code(p.check_write(&t.out.join("..").join("other").join("x.png"), "path", watched)), "path_denied");
    // Exports and Save As: a write root or a watched folder (they join the library), never
    // `.dalica-originals`, never over an existing file.
    assert_eq!(code(p.check_output(&t.out.join("Exports").join("deep"), "dir", watched, Output::Folder)), "ok", "made when missing");
    assert_eq!(code(p.check_output(&t.photos, "dir", watched, Output::Folder)), "ok");
    assert_eq!(code(p.check_output(&t.photos.join(ORIGINALS).join("x"), "dir", watched, Output::Folder)), "path_denied");
    assert_eq!(code(p.check_output(&t.other.join("Exports"), "dir", watched, Output::Folder)), "path_denied");
    assert_eq!(code(p.check_output(&t.photos.join("taken.jpg"), "dir", watched, Output::Folder)), "bad_params");
    assert_eq!(code(p.check_output(&t.photos.join("new.jpg"), "path", watched, Output::File)), "ok");
    assert_eq!(code(p.check_output(&t.photos.join("taken.jpg"), "path", watched, Output::File)), "path_denied", "an existing file");
    assert_eq!(code(p.check_output(&t.out.join("taken.png"), "path", watched, Output::File)), "path_denied", "an existing file");
    // `.dalica-originals` in another case is the same folder on a case-insensitive volume.
    std::fs::create_dir_all(t.out.join(".Dalica-Originals")).unwrap();
    assert_eq!(code(p.check_output(&t.out.join(".Dalica-Originals").join("a.jpg"), "path", watched, Output::File)), "path_denied");
    #[cfg(unix)]
    {
        // A link inside the write root that points out of it is followed before the check.
        std::os::unix::fs::symlink(&t.other, t.out.join("link")).unwrap();
        assert_eq!(code(p.check_write(&t.out.join("link").join("x.png"), "path", watched)), "path_denied");
        assert_eq!(code(p.check_output(&t.out.join("link").join("Exports"), "dir", watched, Output::Folder)), "path_denied");
        assert_eq!(code(p.check_output(&t.out.join("link").join("a.jpg"), "path", watched, Output::File)), "path_denied");
        // A link into a watched folder: the screenshot would become a library photo.
        std::os::unix::fs::symlink(&t.photos, t.out.join("into-photos")).unwrap();
        assert_eq!(code(p.check_write(&t.out.join("into-photos").join("x.png"), "path", watched)), "path_denied");
    }
    // Without a write root, the defaults: the temporary folder (and Dalica's automation folder).
    assert!(Policy::default().write_roots().contains(&std::env::temp_dir()));
    assert_eq!(automation::default_write_roots().first(), Some(&std::env::temp_dir()));
}

#[test]
fn client_roots_are_read_roots_only_and_a_new_list_replaces_the_old() {
    let t = tree();
    // An explicit write root: the default ones (the temporary folder) hold this whole tree.
    let mut p = Policy::with_roots(&[], std::slice::from_ref(&t.out));
    assert_eq!(p.set_client_roots(vec![t.other.clone(), PathBuf::from("relative"), t.other.clone()]), 1, "relative dropped, duplicates once");
    assert_eq!(code(p.check_read(&t.other, "path", &[])), "ok");
    assert_eq!(code(p.check_write(&t.other.join("x.png"), "path", &[])), "path_denied", "never a write root");
    assert_eq!(code(p.check_output(&t.other.join("Exports"), "dir", &[], Output::Folder)), "path_denied");
    p.set_client_roots(vec![t.extra.clone()]);
    assert_eq!(code(p.check_read(&t.other, "path", &[])), "path_denied", "a removed root is gone");
    assert_eq!(code(p.check_read(&t.extra, "path", &[])), "ok");
    assert!(!p.add_read(PathBuf::from("relative")), "a configured root must be absolute too");
}

#[test]
fn effects_outside_dalica_need_their_permission() {
    let mut p = Policy::default();
    assert_eq!(p.check_effect(Effect::External, "map.allow").unwrap_err().kind, Refusal::ExternalDenied);
    assert!(p.check_effect(Effect::Library, "photo.star.toggle").is_ok() && p.check_effect(Effect::Files, "export.run").is_ok());
    p.set_allow_external(true);
    assert!(p.check_effect(Effect::External, "map.allow").is_ok());
    assert!(!p.allow_people());
    p.set_allow_people(true);
    assert!(p.allow_people());
}

#[test]
fn names_windows_would_misread_are_named() {
    for bad in ["a.png:stream", "C:", "photo.", "photo ", "NUL", "nul.txt", "COM1", "lpt9.jpg", "CONOUT$"] {
        assert!(automation::windows_problem(bad).is_some(), "{bad}");
    }
    for good in ["photo.png", "COM10", "Console", "LPT", "a b", ".dalica-originals"] {
        assert_eq!(automation::windows_problem(good), None, "{good}");
    }
    let long = PathBuf::from("/").join("a".repeat(automation::MAX_PATH + 1));
    assert_eq!(code(automation::lexical(&long, "path")), "bad_params");
    #[cfg(windows)]
    {
        // Verbatim and device prefixes skip Windows' own checks: refused before any I/O.
        for raw in [r"\\?\C:\photos", r"\\.\C:\photos", r"\\?\UNC\server\share\x"] {
            assert_eq!(code(automation::lexical(Path::new(raw), "path")), "path_denied", "{raw}");
        }
        assert_eq!(code(automation::lexical(Path::new(r"C:\photos\NUL"), "path")), "bad_params");
        assert_eq!(code(automation::lexical(Path::new(r"C:\out\a.png:x"), "path")), "bad_params");
    }
}

/// `options.set` changing an option that lets the app reach outside or decides where it connects
/// needs `--automation-allow-external` (M13.28); turning tiles off and the attribution do not.
#[test]
fn options_that_reach_outside_need_the_external_permission() {
    let mut p = Policy::default();
    let set = |values: serde_json::Value| serde_json::Map::from_iter([("values".to_string(), values)]);
    for key in dalica_engine::automation::EXTERNAL_OPTIONS {
        let value = if *key == "network.allowMapTiles" { serde_json::json!(true) } else { serde_json::json!("x") };
        let refused = p.check_command(Effect::Library, "options.set", &set(serde_json::json!({ *key: value }))).unwrap_err();
        assert_eq!(refused.kind, Refusal::ExternalDenied, "{key}");
    }
    for fine in [
        serde_json::json!({ "network.allowMapTiles": false }),
        serde_json::json!({ "network.tileAttribution": "© Maps", "interface.animations": false }),
    ] {
        assert!(p.check_command(Effect::Library, "options.set", &set(fine.clone())).is_ok(), "{fine}");
    }
    assert!(
        p.check_command(Effect::Library, "photo.star.toggle", &set(serde_json::json!({ "network.allowMapTiles": true }))).is_ok(),
        "only options.set"
    );
    assert_eq!(p.check_command(Effect::External, "map.allow", &serde_json::Map::new()).unwrap_err().kind, Refusal::ExternalDenied);
    p.set_allow_external(true);
    assert!(p.check_command(Effect::Library, "options.set", &set(serde_json::json!({ "network.allowMapTiles": true }))).is_ok());
}
