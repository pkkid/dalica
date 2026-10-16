//! The edit view's default keys that `editing.md` §12 documents and the keymap binds there
//! (`menus-and-shortcuts.md` §2.0: up to two keys per context).

use dalica_engine::{Context, Key};
use serde_json::json;

use crate::common::{jpeg, lisbon};

/// Space in the edit view is Star / Unstar (its pair there is Ctrl+8 and Space; F4 stays the
/// Library's): it resolves to `photo.star.toggle`, nothing else is bound to Space in the edit view
/// or in Global under it, and running what it resolves to stars the photo, then unstars it.
#[test]
fn space_in_the_edit_view_toggles_the_star() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&[jpeg(1)], None);
    let space = Key::parse("Space").unwrap();
    let star = s.keymap().resolve(&space, Context::EditView, false);
    assert_eq!(star.as_deref(), Some("photo.star.toggle"));
    let on_space: Vec<String> = s
        .keymap()
        .bindings()
        .into_iter()
        .filter(|b| b.key == space && matches!(b.context, Context::EditView | Context::Global))
        .map(|b| b.command)
        .collect();
    assert_eq!(on_space, ["photo.star.toggle"], "no other default on Space in the edit view");
    let keys: Vec<String> = s.keymap().keys_for("photo.star.toggle", Context::EditView).iter().map(Key::to_string).collect();
    assert_eq!(keys, ["Ctrl+8", "Space"]);
    assert_eq!(s.keymap().resolve(&space, Context::Library, false), None, "Space is the edit view's only");

    let id = star.unwrap();
    s.execute(&id, &json!({ "ids": [ids[0]] })).unwrap();
    assert!(s.library().photo(ids[0]).unwrap().star, "starred");
    s.execute(&id, &json!({ "ids": [ids[0]] })).unwrap();
    assert!(!s.library().photo(ids[0]).unwrap().star, "unstarred again");
}
