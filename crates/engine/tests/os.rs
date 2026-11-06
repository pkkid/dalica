//! Locate on Disk, Open With, Open in External Editor and the system clipboard (M15.17,
//! `organizing.md` §9, `security.md` §5.11) through the recording `Os`: the commands hand the
//! service the right paths and handler, in order, with the owner thread making no file call; the
//! launching ones never run over automation; a headless session has no clipboard; a missing file
//! answers why before the system is asked; the picture copied is the photo's edited picture.
//! Open With ▸'s icons (M16.23) are read off the owner thread, once, and a bad one is none.

use std::path::PathBuf;
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::automation::{Policy, Refusal};
use dalica_engine::cmd::os::{ASK_ABOVE, MAX_EDITOR_IDS, OFFLINE, missing};
use dalica_engine::handler_icons::HandlerIcon;
use dalica_engine::os::{Clip, Handler, IconFile, NO_CLIPBOARD, OsCall, OsError, Recording, Service};
use dalica_engine::{CmdError, Session};
use serde_json::{Value, json};

use crate::common::{jpeg, lisbon, off_owner, wait_files};

/// A library of three photos in `Lisbon`, with a windowed recording `Os` plugged in.
fn setup() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, Vec<PhotoId>, Arc<Recording>) {
    let (lib, disk, folder, mut s, ids) = lisbon(&[jpeg(1), jpeg(2), jpeg(3)], None);
    let os = Arc::new(Recording::windowed());
    s.set_os(os.clone());
    (lib, disk, folder, s, ids, os)
}

fn handler(id: &str, name: &str) -> Handler {
    Handler::new(id, name)
}

#[test]
fn each_command_hands_the_right_paths_and_handler_in_order() {
    let (_lib, _disk, folder, mut s, ids, os) = setup();
    let (a, b, c) = (folder.join("IMG_0001.jpg"), folder.join("IMG_0002.jpg"), folder.join("IMG_0003.jpg"));
    let dalica = handler("org.dalica.Dalica.desktop", "Dalica");
    let viewer = handler("org.example.Viewer.desktop", "Image Viewer");
    let paint = handler("org.example.Paint.desktop", "Paint");
    os.set_handlers("jpg", vec![viewer.clone(), dalica, paint.clone()]);
    s.change_settings(|st| st.set_external_editor("/opt/Editor Pro/bin/editor"));
    let folder_id = s.library().photo(ids[0]).unwrap().folder;

    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    assert_eq!(s.execute("photo.locate", &json!({ "id": ids[0] })).unwrap(), json!({ "path": a }));
    assert_eq!(s.execute("folder.locate", &json!({ "id": folder_id })).unwrap(), json!({ "path": folder }));
    let listed = s.execute("photo.openWithList", &json!({ "id": ids[1] })).unwrap();
    assert_eq!(
        listed["handlers"],
        json!([{ "id": viewer.id, "name": viewer.name }, { "id": paint.id, "name": paint.name }]),
        "the system's order, Dalica left out"
    );
    s.execute("photo.openWith", &json!({ "id": ids[1], "handler": paint.id })).unwrap();
    let opened = s.execute("photo.openExternal", &json!({ "ids": [ids[2], ids[0], ids[2]] })).unwrap();
    assert_eq!(opened["editor"], json!("/opt/Editor Pro/bin/editor"));
    s.execute("photo.copyToSystem", &json!({ "ids": [ids[0], ids[1]] })).unwrap();
    let copied = s.execute("photo.copyPath", &json!({ "ids": [ids[0], ids[1], ids[2]] })).unwrap();
    let text = [&a, &b, &c].map(|p| p.to_string_lossy().into_owned()).join("\n");
    assert_eq!(copied["text"], json!(text), "three paths, three lines");
    assert_eq!(text.lines().count(), 3);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);

    assert_eq!(
        os.calls(),
        [
            OsCall::Reveal(a.clone()),
            OsCall::Reveal(folder.clone()),
            OsCall::Handlers(b.clone()),
            OsCall::OpenWith { path: b.clone(), handler: paint.id.clone() },
            OsCall::OpenInEditor { paths: vec![c.clone(), a.clone()], editor: "/opt/Editor Pro/bin/editor".into() },
            OsCall::PutOnClipboard(Clip::Files(vec![a.clone(), b.clone()])),
            OsCall::PutOnClipboard(Clip::Text(text)),
        ]
    );
}

/// The launching commands are never automated (`security.md` §6.6), not even with
/// `--automation-allow-external`: an automation surface refuses them before the engine sees them,
/// so nothing reaches the system; the copy commands and the list are no launches.
#[test]
fn over_automation_the_launching_commands_are_refused_and_nothing_is_recorded() {
    let (_lib, _disk, _folder, mut s, ids, os) = setup();
    let folder_id = s.library().photo(ids[0]).unwrap().folder;
    let mut policy = Policy::default();
    let launching = [
        ("photo.locate", json!({ "id": ids[0] })),
        ("folder.locate", json!({ "id": folder_id })),
        ("photo.openWith", json!({ "id": ids[0], "handler": "org.example.Viewer.desktop" })),
        ("photo.openExternal", json!({ "ids": [ids[0]] })),
    ];
    for (id, params) in &launching {
        let spec = *s.spec(id).unwrap();
        let refusal = policy.check_command(spec.effect, spec.id, params.as_object().unwrap()).unwrap_err();
        assert_eq!(refusal.kind, Refusal::ExternalDenied, "{id}");
        assert!(dalica_engine::automation::never_automated(id).is_err(), "{id}: every surface asks this first");
    }
    // `security.md` §6.6: launching programs is refused over automation whatever was granted.
    policy.set_allow_external(true);
    for (id, params) in &launching {
        let spec = *s.spec(id).unwrap();
        assert!(policy.check_command(spec.effect, spec.id, params.as_object().unwrap()).is_err(), "{id} with --automation-allow-external");
    }
    assert!(os.calls().is_empty(), "nothing reached the system");
    for id in ["photo.openWithList", "photo.copyToSystem", "photo.copyPath"] {
        let spec = *s.spec(id).unwrap();
        assert!(policy.check_command(spec.effect, spec.id, &serde_json::Map::new()).is_ok(), "{id}");
    }
    // The person at the window runs them.
    s.execute("photo.locate", &json!({ "id": ids[0] })).unwrap();
    assert_eq!(os.calls().len(), 1);
}

/// A headless session (the session's own recording `Os`) has no clipboard: the copy commands are
/// disabled with the reason and nothing is asked; an OS that cannot do a launch greys it alike.
#[test]
fn a_headless_session_has_no_clipboard() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&[jpeg(1)], None);
    for (id, params) in [("photo.copyToSystem", json!({ "ids": [ids[0]] })), ("photo.copyPath", json!({ "ids": [ids[0]] }))] {
        assert_eq!(s.execute(id, &params), Err(CmdError::Disabled(NO_CLIPBOARD.into())), "{id}");
    }
    let os = Arc::new(Recording::windowed());
    os.set_unavailable(Service::Reveal, Some("No file manager selects files here."));
    s.set_os(os.clone());
    assert_eq!(s.execute("photo.locate", &json!({ "id": ids[0] })), Err(CmdError::Disabled("No file manager selects files here.".into())));
    assert!(os.calls().is_empty());
    // A failure the system reports is the command's, with its reason.
    os.fail_with(Some(OsError::Failed("The clipboard is held by another application.".into())));
    assert_eq!(
        s.execute("photo.copyPath", &json!({ "ids": [ids[0]] })),
        Err(CmdError::Disabled("The clipboard is held by another application.".into()))
    );
}

/// A file gone from its folder answers why, and the system is never asked; an offline folder too.
#[test]
fn a_missing_file_answers_its_reason_before_the_system_is_asked() {
    let (_lib, _disk, folder, mut s, ids, os) = setup();
    let gone = folder.join("IMG_0002.jpg");
    let target = gone.clone();
    off_owner(move || std::fs::remove_file(target).unwrap());
    let reason = CmdError::NotFound(missing(&gone));
    assert_eq!(s.execute("photo.locate", &json!({ "id": ids[1] })), Err(reason.clone()));
    assert_eq!(s.execute("photo.openExternal", &json!({ "ids": [ids[0], ids[1]] })), Err(reason.clone()), "nothing opens");
    assert_eq!(s.execute("photo.copyToSystem", &json!({ "ids": [ids[1]] })), Err(reason));
    assert!(os.calls().is_empty(), "the system was not asked");

    // The picture's job fails its item with the same reason.
    s.execute("photo.copyToSystem", &json!({ "ids": [ids[1]], "picture": true })).unwrap();
    wait_files(&mut s);
    let event = s.file_job_events().last().unwrap().clone();
    assert_eq!(event.failures.first().map(|f| f.reason.clone()), Some(missing(&gone)));
    assert!(os.calls().is_empty());

    // An offline folder is refused from the library alone.
    s.set_online_check(Some(Arc::new(|_: &std::path::Path| false)));
    assert!(s.refresh_volumes(std::time::Duration::from_secs(60)));
    assert_eq!(s.execute("photo.locate", &json!({ "id": ids[0] })), Err(CmdError::Disabled(OFFLINE.into())));
}

#[test]
fn a_photo_with_a_crop_and_an_effect_copies_a_picture_of_its_edited_size() {
    let (_lib, _disk, _folder, mut s, ids, os) = setup();
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "crop(0,0,0.5,0.5)" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "sepia()" })).unwrap();
    let reply = s.execute("photo.copyToSystem", &json!({ "ids": [ids[0]], "picture": true })).unwrap();
    assert_eq!((reply["pending"].clone(), reply["total"].clone()), (json!(true), json!(1)));
    wait_files(&mut s);
    let event = s.file_job_events().last().unwrap().clone();
    assert!(event.failures.is_empty(), "{:?}", event.failures);
    match os.calls().as_slice() {
        [OsCall::PutOnClipboard(Clip::Picture { width, height, rgba })] => {
            assert_eq!((*width, *height), (48, 32), "the 96 × 64 photo cropped to its top-left quarter");
            assert_eq!(rgba.len(), 48 * 32 * 4);
            let (r, g, b) = (rgba[0], rgba[1], rgba[2]);
            assert!(r >= g && g >= b, "sepia: warm ({r}, {g}, {b})");
        }
        other => panic!("one picture: {other:?}"),
    }
}

/// Each command's contract: what it refuses before anything happens.
#[test]
fn each_command_refuses_what_it_cannot_take() {
    let (_lib, _disk, _folder, mut s, ids, os) = setup();
    let mut code = |id: &str, p: Value| s.execute(id, &p).unwrap_err().code();
    assert_eq!(code("photo.locate", json!({ "id": 999_999 })), "not_found");
    assert_eq!(code("folder.locate", json!({ "id": 999_999 })), "not_found");
    assert_eq!(code("photo.locate", json!({})), "bad_params");
    assert_eq!(code("photo.openWith", json!({ "id": ids[0], "handler": "" })), "bad_params");
    assert_eq!(code("photo.openWith", json!({ "id": ids[0], "handler": "a\nb" })), "bad_params");
    assert_eq!(code("photo.openWith", json!({ "id": ids[0], "handler": "x".repeat(1025) })), "bad_params");
    assert_eq!(code("photo.copyPath", json!({ "ids": [] })), "bad_params");
    assert_eq!(code("photo.copyToSystem", json!({ "ids": [ids[0], ids[1]], "picture": true })), "bad_params");
    let many: Vec<PhotoId> = vec![ids[0]; MAX_EDITOR_IDS + 1];
    assert_eq!(code("photo.openExternal", json!({ "ids": many })), "limit");
    assert!(os.calls().is_empty());
}

/// More than twenty photos ask before they open in the external editor.
#[test]
fn more_than_twenty_photos_ask_first() {
    let photos: Vec<Vec<u8>> = (0..=ASK_ABOVE as u64).map(jpeg).collect();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos, None);
    let os = Arc::new(Recording::windowed());
    s.set_os(os.clone());
    match s.execute("photo.openExternal", &json!({ "ids": ids })) {
        Err(CmdError::Disabled(m)) => assert!(m.contains("Open 21 pictures in the external editor?") && m.contains("confirm"), "{m}"),
        other => panic!("asks first: {other:?}"),
    }
    assert!(os.calls().is_empty());
    s.execute("photo.openExternal", &json!({ "ids": ids[..ASK_ABOVE] })).unwrap();
    s.execute("photo.openExternal", &json!({ "ids": ids, "confirm": true })).unwrap();
    assert_eq!(os.calls().len(), 2);
}

/// Open With ▸'s icons (M16.23): a planted 32 px PNG comes back as a 16 px picture, read off the
/// owner thread and once only; a damaged file, a 50 MB one and one that is not there are none,
/// never a panic.
#[test]
fn a_handlers_icon_is_read_off_the_owner_thread_once_and_a_bad_one_is_none() {
    let (_lib, disk, _folder, mut s, ids, os) = setup();
    let icons = disk.path().join("icons");
    let red: Vec<u8> = (0..32 * 32).flat_map(|_| [200u8, 30, 30, 255]).collect();
    let (good, damaged, huge) = (icons.join("viewer.png"), icons.join("damaged.png"), icons.join("huge.png"));
    off_owner(|| {
        std::fs::create_dir_all(&icons).unwrap();
        std::fs::write(&good, dalica_scenes::fingerprint::png_rgba(32, 32, &red)).unwrap();
        std::fs::write(&damaged, b"\x89PNG\r\n\x1a\nnot a picture at all").unwrap();
        std::fs::File::create(&huge).unwrap().set_len(50 * 1024 * 1024).unwrap();
    });
    for (icon, path) in [("viewer", &good), ("damaged", &damaged), ("huge", &huge), ("gone", &icons.join("gone.png"))] {
        os.set_icon_file(icon, IconFile::Path(path.clone()));
    }
    let with_icon = |id: &str, icon: &str| Handler { icon: Some(icon.into()), ..handler(id, id) };
    let apps = [
        with_icon("viewer", "viewer"),
        with_icon("damaged", "damaged"),
        with_icon("huge", "huge"),
        with_icon("gone", "gone"),
        handler("plain", "Plain"),
    ];
    os.set_handlers("jpg", apps.to_vec());
    let listed = s.execute("photo.openWithList", &json!({ "id": ids[0] })).unwrap();
    assert_eq!(listed["handlers"][0], json!({ "id": "viewer", "name": "viewer", "icon": "viewer" }));
    os.clear();

    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let answers = loop {
        let answers: Vec<HandlerIcon> = apps.iter().map(|h| s.handler_icon(&h.id, 16)).collect();
        if !answers.contains(&HandlerIcon::Loading) || std::time::Instant::now() > deadline {
            break answers;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread read no file");
    let HandlerIcon::Ready(picture) = &answers[0] else { panic!("the PNG: {:?}", answers[0]) };
    assert_eq!((picture.width, picture.height), (16, 16));
    assert!(picture.data.as_chunks::<4>().0.iter().all(|p| *p == [200, 30, 30, 255]), "its pixels");
    assert_eq!(&answers[1..], [HandlerIcon::None, HandlerIcon::None, HandlerIcon::None, HandlerIcon::None], "damaged, 50 MB, gone, none given");
    // Asked again: the same picture, the system not asked again.
    assert_eq!(s.handler_icon("viewer", 16), HandlerIcon::Ready(picture.clone()));
    assert_eq!(s.handler_icon("damaged", 16), HandlerIcon::None);
    drop(ui);
    let mut asked: Vec<String> =
        os.calls().into_iter().filter_map(|c| if let OsCall::IconFile { icon, px: 16 } = c { Some(icon) } else { None }).collect();
    asked.sort();
    assert_eq!(asked, ["damaged", "gone", "huge", "viewer"], "each file found once");
}
