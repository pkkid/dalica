//! Remove from Album (`organizing.md` §4.3): asked through the host before `album.remove` runs (a
//! row of [`super::confirm::ASKS`]), "Remove the %d selected pictures from this album? The files
//! are not deleted." with [Remove] [Cancel]. Remove (the default) runs `album.remove` with
//! `"confirm": true`. Inside an album the Delete key and the photo menu's Remove from Album ask it
//! (`crate::keys`, the photo context menu).
//!
//! It asks once `album.remove` is registered (M9.10) for a user album, unless the engine's "Remove
//! from albums without asking" (`organize.removeFromAlbumsWithoutAsking`) is on: then the photos
//! leave the album at once, as the engine runs it without `confirm` too. A missing or automatic
//! album is the command's to refuse, without a question. The question is the generic
//! confirmation's ([`super::confirm`]); this module says what it asks.

use dalica_engine::CmdError;
use egui::Ui;
use serde_json::Value;

use super::{Dialog, Look, confirm};
use crate::app::App;

/// The dialog's title.
pub const TITLE: &str = "Remove from Album";
/// Its default button.
pub const REMOVE: &str = "Remove";
/// The question when the number of photos is not known.
pub const TEXT: &str = "Remove the selected pictures from this album? The files are not deleted.";

/// Whether `album.remove` with `params` asks first: once the command exists, for a user album
/// (`params.album`), unless "Remove from albums without asking" is on.
pub fn asks(app: &App, params: &Value) -> bool {
    let user_album = params.get("album").and_then(Value::as_str).is_some_and(|a| dalica_core::AlbumId::parse(a).is_ok());
    confirm::registered(app, "album.remove") && user_album && !without_asking(app)
}

/// Whether Options' "Remove from albums without asking" is on.
pub fn without_asking(app: &App) -> bool {
    app.session().is_some_and(|s| s.settings().organize().remove_from_albums_without_asking)
}

/// The question for the photos `params.ids` names (else the tray's).
pub fn text_for(app: &App, params: &Value) -> Option<String> {
    let n = params.get("ids").and_then(Value::as_array).map_or_else(|| crate::tray::photos(app.state()).len(), Vec::len);
    Some(question(n))
}

/// "Remove the %d selected pictures from this album? The files are not deleted." (one picture:
/// "the selected picture").
pub fn question(n: usize) -> String {
    match n {
        1 => "Remove the selected picture from this album? The file is not deleted.".into(),
        n => format!("Remove the {n} selected pictures from this album? The files are not deleted."),
    }
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    confirm::look(d)
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    confirm::body(app, ui, d);
}

pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    confirm::act(app, d, action)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::dialogs::{self, DialogKind};

    #[test]
    fn the_question_counts_the_photos_and_asks_only_once_the_command_exists() {
        assert_eq!(question(1), "Remove the selected picture from this album? The file is not deleted.");
        assert_eq!(question(3), "Remove the 3 selected pictures from this album? The files are not deleted.");
        let mut app = App::new(None);
        assert!(!asks(&app, &json!({ "album": "a1234", "ids": [1] })), "no library, no command: no question");
        assert!(!without_asking(&app));
        assert_eq!(text_for(&app, &json!({ "ids": [1, 2] })).as_deref(), Some(question(2).as_str()));
        let ask = confirm::of(DialogKind::RemoveFromAlbum).unwrap();
        assert_eq!(confirm::prompt(ask), json!({ "title": TITLE, "text": TEXT, "buttons": ["Remove", "Cancel"], "dialog": "removeFromAlbum" }));
        // Opened as the host opens it: the generic question, Remove the default button.
        dialogs::ask(&mut app, ask, &json!({ "album": "a1234", "ids": [1, 2] })).unwrap();
        let v = dialogs::inspect(&app);
        assert_eq!((v["kind"].as_str(), v["default"].as_str()), (Some("removeFromAlbum"), Some("button:dialog.remove")));
        assert_eq!(confirm::question(&app, dialogs::top(&app).unwrap()), question(2));
        dialogs::cancel(&mut app).unwrap();
        assert!(app.state().dialogs.is_empty());
    }

    #[test]
    fn it_asks_for_a_user_album_unless_the_setting_says_not_to() {
        let (_dir, s, _ids) = crate::app::tests::session_with_photos(1);
        let mut app = App::new(Some(s));
        assert!(asks(&app, &json!({ "album": "a1234", "ids": [1] })));
        for (params, why) in [
            (json!({ "album": "starred", "ids": [1] }), "an automatic album"),
            (json!({ "ids": [1] }), "no album"),
            (json!({ "album": 7 }), "a bad album"),
        ] {
            assert!(!asks(&app, &params), "{why}: the command refuses it");
        }
        let session = app.session_mut().unwrap();
        let mut o = session.settings().organize();
        o.remove_from_albums_without_asking = true;
        session.change_settings(|st| st.set_organize(&o));
        assert!(without_asking(&app) && !asks(&app, &json!({ "album": "a1234", "ids": [1] })));
    }
}
