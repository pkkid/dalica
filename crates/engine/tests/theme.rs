//! The theme commands (`roadmap.md` M16.4, `cmd/theme.rs`): one `theme.<id>` per theme under
//! View ▸ Theme ▸, each writing its id to `settings.json` and checked while it is in effect, with no
//! undo step; `theme.next` goes round every theme. The write itself (atomic, a read-only file
//! refused) is `options.set`'s, tested in `options_set.rs`; hostile parameters are the
//! every-command loop's and `panic_hunt`'s.

use dalica_engine::cmd::theme::{checked, command};
use dalica_engine::settings::{FILE, THEMES};
use serde_json::json;

use crate::common::{open, read_json};

#[test]
fn each_theme_command_writes_its_id_and_is_checked_while_in_effect() {
    let lib = tempfile::tempdir().unwrap();
    let mut s = open(lib.path());
    assert_eq!(s.settings().interface().theme, "dark", "Dark until one is chosen");
    assert_eq!(checked(&s, "theme.dark"), Some(true));
    assert_eq!((checked(&s, "theme.next"), checked(&s, "view.colorManagement")), (None, None), "not theme rows");
    for (id, label) in THEMES {
        let cmd = command(id).unwrap();
        assert_eq!(cmd, format!("theme.{id}"));
        let spec = s.spec(cmd).unwrap();
        assert_eq!((spec.label, spec.menu, spec.keys.len()), (*label, format!("View/Theme/{label}").as_str(), 0), "{id}");
        let answer = s.execute(cmd, &json!({})).unwrap();
        assert_eq!(answer, json!({ "theme": id, "changed": *id != "dark" }), "{id}");
        assert_eq!(s.settings().interface().theme, *id);
        for (other, _) in THEMES {
            assert_eq!(checked(&s, &format!("theme.{other}")), Some(other == id), "{other} while {id} is in effect");
        }
        s.flush().unwrap();
        assert_eq!(read_json(&lib.path().join(FILE)).unwrap()["interface"]["theme"], json!(id), "{id} is on disk");
    }
    assert_eq!(s.execute("theme.breezeDark", &json!(null)).unwrap(), json!({ "theme": "breezeDark", "changed": false }), "again: no change");
    assert!(s.undo_stack().is_empty(), "a theme is not an undo step");
    assert_eq!(command("Dark"), None, "ids are exact");
    drop(s);
    assert_eq!(open(lib.path()).settings().interface().theme, "breezeDark", "kept over a restart");
}

#[test]
fn theme_next_goes_round_every_theme() {
    let lib = tempfile::tempdir().unwrap();
    let mut s = open(lib.path());
    let mut seen = Vec::new();
    for _ in THEMES {
        let answer = s.execute("theme.next", &json!({})).unwrap();
        seen.push(answer["theme"].as_str().unwrap().to_string());
    }
    let ids: Vec<&str> = THEMES.iter().map(|(id, _)| *id).collect();
    let mut want = ids[1..].to_vec();
    want.push("dark");
    assert_eq!(seen, want, "after Dark the list in order, then Dark again");
    assert_eq!(s.settings().interface().theme, "dark");
    assert_eq!(s.spec("theme.next").unwrap().menu, "", "for scripts, not the menu");
}
