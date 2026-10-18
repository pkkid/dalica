//! "Name?" with ✓ / × on Unnamed faces that have a suggestion (`organizing.md` §6.2–§6.3,
//! `faces.md` §5): the grid's tiles read the window's copy of the suggestions
//! (`crate::commands::suggestions`); ✓ confirms the face as its suggested person
//! (`suggestions.confirm`, one undo step: it leaves the grid), × rejects the pair for good
//! (`suggestions.reject`: the face stays, without a suggestion).

use serde_json::{Value, json};

use crate::app::App;
use crate::state::FaceRef;

/// The name suggested for `face` (shown as "Name?"), if it has a suggestion.
pub fn name(app: &App, face: FaceRef) -> Option<String> {
    let snapshot = crate::commands::suggestions::latest();
    let person = snapshot.suggested(face)?;
    Some(app.session()?.library().person(person)?.name.clone())
}

/// The command a tile's ✓ (`confirm`) or × (`reject`) asks for.
pub fn act(face: FaceRef, confirm: bool, reject: bool) -> Option<(&'static str, Value)> {
    let id = if confirm {
        "suggestions.confirm"
    } else if reject {
        "suggestions.reject"
    } else {
        return None;
    };
    Some((id, json!({ "faces": [face] })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marks_ask_for_the_face_they_sit_under() {
        let face = FaceRef { photo: dalica_core::PhotoId::new(12), index: 1 };
        assert_eq!(act(face, true, false), Some(("suggestions.confirm", json!({ "faces": ["12.1"] }))));
        assert_eq!(act(face, false, true), Some(("suggestions.reject", json!({ "faces": ["12.1"] }))));
        assert_eq!(act(face, false, false), None);
        assert_eq!(name(&App::new(None), face), None);
    }
}
