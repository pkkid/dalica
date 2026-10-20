//! Dalica as the default viewer (M15.24, `viewing.md` §3.3) through the recording `Os`:
//! `viewer.fileTypes` lists the types this build shows with the fake's handlers; setting three
//! hands the system exactly those three and remembers them in `viewer.defaultViewer`; none is
//! "Don't use Dalica as a viewer"; a type not offered is refused before the system is asked; the
//! system's own refusal is `disabled` with its reason; the command needs
//! `--automation-allow-external`; `photo.openWithList` gives the handlers' icons.

use std::sync::Arc;

use dalica_engine::automation::Policy;
use dalica_engine::os::{Handler, OsCall, OsError, Recording, Service, viewer_file_types};
use dalica_engine::{CmdError, Session};
use dalica_scan::FileType;
use serde_json::json;

use crate::common::{jpeg, lisbon};

fn session() -> (tempfile::TempDir, Session, Arc<Recording>) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let os = Arc::new(Recording::windowed());
    s.set_os(os.clone());
    (dir, s, os)
}

#[test]
fn the_types_come_with_their_handlers_and_setting_three_records_three() {
    let (_dir, mut s, os) = session();
    let viewer = Handler { id: "org.example.Viewer.desktop".into(), name: "Image Viewer".into(), icon: Some("org.example.Viewer".into()) };
    os.set_default(FileType::Jpeg, viewer.clone());
    os.set_default(FileType::Png, Handler::new("org.dalica.Dalica.desktop", "Dalica"));
    let listed = s.execute("viewer.fileTypes", &json!({})).unwrap();
    let types = listed["types"].as_array().unwrap();
    let offered = viewer_file_types();
    assert_eq!(types.len(), offered.len());
    assert_eq!(types.iter().map(|t| t["type"].as_str().unwrap()).collect::<Vec<_>>(), offered.iter().map(|t| t.id()).collect::<Vec<_>>());
    assert_eq!(
        types[0],
        json!({ "type": "jpeg", "name": "JPEG", "handler": { "id": viewer.id, "name": "Image Viewer", "icon": "org.example.Viewer" }, "dalica": false })
    );
    assert_eq!(types[1]["dalica"], true, "Dalica holds PNG");
    assert_eq!(types[2]["handler"], json!(null), "the system does not say");
    assert_eq!(listed["chosen"], json!([]));
    assert!(!offered.contains(&FileType::Avif), "AVIF is not shown by this build");
    assert_eq!(offered.contains(&FileType::Heif), dalica_image::Format::Heif.supported());
    assert!(os.calls().iter().all(|c| matches!(c, OsCall::DefaultHandler(_))));

    os.clear();
    // Repeats once, in the catalogue's order.
    let set = s.execute("viewer.setDefault", &json!({ "types": ["raw", "jpeg", "png", "jpeg"] })).unwrap();
    assert_eq!(set, json!({ "types": ["jpeg", "png", "raw"], "set": true }));
    assert_eq!(os.calls(), [OsCall::SetDefaultViewer(vec![FileType::Jpeg, FileType::Png, FileType::Raw])]);
    assert_eq!(s.settings().viewer().default_viewer, [FileType::Jpeg, FileType::Png, FileType::Raw]);
    assert_eq!(s.execute("viewer.fileTypes", &json!({})).unwrap()["chosen"], json!(["jpeg", "png", "raw"]));

    // "Don't use Dalica as a viewer".
    os.clear();
    assert_eq!(s.execute("viewer.setDefault", &json!({ "types": [] })).unwrap(), json!({ "types": [], "set": true }));
    assert_eq!(os.calls(), [OsCall::SetDefaultViewer(Vec::new())]);
    assert!(s.settings().viewer().default_viewer.is_empty());

    // Bad types never reach the system; its refusal is `disabled` and changes no setting.
    os.clear();
    for bad in [json!({ "types": ["video"] }), json!({ "types": ["avif"] }), json!({ "types": "jpeg" }), json!({})] {
        assert!(matches!(s.execute("viewer.setDefault", &bad), Err(CmdError::BadParams(_))), "{bad}");
    }
    assert!(os.calls().is_empty());
    os.fail_with(Some(OsError::Failed("mimeapps.list was not written".into())));
    assert_eq!(s.execute("viewer.setDefault", &json!({ "types": ["gif"] })), Err(CmdError::Disabled("mimeapps.list was not written".into())));
    assert!(s.settings().viewer().default_viewer.is_empty());
    os.fail_with(None);
    os.set_unavailable(Service::DefaultViewer, Some("No way here"));
    assert!(matches!(s.execute("viewer.fileTypes", &json!({})), Err(CmdError::Disabled(r)) if r == "No way here"));

    // It changes the system's settings: automation needs --automation-allow-external.
    let spec = *s.spec("viewer.setDefault").unwrap();
    let mut policy = Policy::default();
    assert!(policy.check_command(spec.effect, spec.id, &serde_json::Map::new()).is_err());
    policy.set_allow_external(true);
    assert!(policy.check_command(spec.effect, spec.id, &serde_json::Map::new()).is_ok());
}

#[test]
fn open_with_lists_the_handlers_icons() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&[jpeg(1)], None);
    let os = Arc::new(Recording::windowed());
    s.set_os(os.clone());
    let viewer = Handler { id: "viewer.desktop".into(), name: "Image Viewer".into(), icon: Some("/usr/share/icons/viewer.png".into()) };
    os.set_handlers("jpg", vec![viewer, Handler::new("paint.desktop", "Paint")]);
    let listed = s.execute("photo.openWithList", &json!({ "id": ids[0] })).unwrap();
    assert_eq!(
        listed["handlers"],
        json!([{ "id": "viewer.desktop", "name": "Image Viewer", "icon": "/usr/share/icons/viewer.png" }, { "id": "paint.desktop", "name": "Paint" }])
    );
}
