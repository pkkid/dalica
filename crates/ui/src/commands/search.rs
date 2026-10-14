//! Search in the UI (`organizing.md` §8): the view state's `search` (the field's text, the
//! autocomplete highlight, the filters, the age slider's position, and the scroll anchor saved
//! when search mode starts) is driven by the toolbar and `ui.set`; the engine runs the query
//! (`search.set`, M9.11) and the toolbar, the filter banner and the light table show it (M9.22).
//!
//! - `search.focus` (Ctrl+F, F3 in the library): puts the keyboard focus in the toolbar's search
//!   field; the toolbar takes the request on its next frame.

use dalica_engine::{CmdError, Context};
use serde_json::{Value, json};

use super::{UiCommand, no_params};
use crate::app::App;

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "search.focus",
    label: "Find",
    menu: "",
    keys: &[(Context::Library, "Ctrl+F"), (Context::Library, "F3")],
    params: "{}: put the keyboard focus in the toolbar's search field",
    run: focus,
}];

/// `search.focus`: asks the toolbar to focus the search field. Replies `{}`.
fn focus(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.state_mut().focus_search = true;
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_focus_asks_the_toolbar_for_the_focus_and_takes_no_params() {
        let mut app = App::new(None);
        assert_eq!(app.run("search.focus", &json!({})).unwrap(), json!({}));
        assert!(app.state().focus_search);
        app.state_mut().focus_search = false;
        for bad in [json!({ "text": "x" }), json!([1]), json!("x")] {
            assert!(matches!(app.run("search.focus", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(!app.state().focus_search, "nothing changed");
    }
}
