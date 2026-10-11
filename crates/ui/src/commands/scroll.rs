//! Scrolling the light table (`ui-layout.md` §5.6): `section.scrollTo` (a tree row's click), and
//! with the speed-control scroll bar its keys (PgUp / PgDn, Ctrl+↑ / Ctrl+↓ to the previous / next
//! section, Ctrl+Home / Ctrl+End). They move [`UiState::scroll`](crate::state::UiState::scroll),
//! the anchor the light table's view follows: when a command writes it, the view eases there over
//! 0.2 s.

use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, parse};
use crate::app::App;
use crate::state::{ScrollAnchor, Section};

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "section.scrollTo",
    label: "Scroll to Section",
    menu: "",
    keys: &[],
    params: r#"{"section": "folder:<FolderId>"}: ease the light table so that section's header is at the top (0.2 s); again = the top of that section"#,
    run: scroll_to,
}];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScrollTo {
    section: Section,
}

fn scroll_to(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: ScrollTo = parse(params)?;
    let Section::Folder(folder) = p.section;
    if !app.session().is_some_and(|s| s.library().folder(folder).is_some()) {
        return Err(CmdError::NotFound(format!("no folder {folder}")));
    }
    app.state_mut().scroll = ScrollAnchor { section: Some(p.section), offset: 0.0 };
    Ok(json!({ "section": p.section }))
}
