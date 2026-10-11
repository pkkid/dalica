//! Scrolling the light table (`ui-layout.md` §5.6, `menus-and-shortcuts.md` §2.2):
//! `section.scrollTo` (a tree row's click) and the scroll keys: PgUp / PgDn (`scroll.pageUp`,
//! `scroll.pageDown`), Ctrl+↑ / Ctrl+↓ to the previous / next section (`scroll.prevSection`,
//! `scroll.nextSection`, which move like the scroll bar's ⇑ / ⇓), Ctrl+Home / Ctrl+End to the top /
//! bottom of the library (`scroll.top`, `scroll.bottom`). They move
//! [`UiState::scroll`](crate::state::UiState::scroll), the anchor the light table's view follows:
//! when a command writes it, the view eases there over 0.2 s. The scroll keys work on the layout of
//! the last frame (the view's height, the sections' tops) and start from the anchor last asked for,
//! so presses made while the view is still easing add up.

use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, no_params, parse};
use crate::app::App;
use crate::panels::light_table::scrollbar::{self, Step};
use crate::state::{ScrollAnchor, Section};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "section.scrollTo",
        label: "Scroll to Section",
        menu: "",
        keys: &[],
        params: r#"{"section": "folder:<FolderId>"}: ease the light table so that section's header is at the top (0.2 s); again = the top of that section"#,
        run: scroll_to,
    },
    UiCommand {
        id: "scroll.pageUp",
        label: "Page Up",
        menu: "",
        keys: &[(Context::Library, "PageUp")],
        params: "{}: scroll the light table up a page (the view's height less one thumbnail row)",
        run: page_up,
    },
    UiCommand {
        id: "scroll.pageDown",
        label: "Page Down",
        menu: "",
        keys: &[(Context::Library, "PageDown")],
        params: "{}: scroll the light table down a page (the view's height less one thumbnail row)",
        run: page_down,
    },
    UiCommand {
        id: "scroll.prevSection",
        label: "Previous Section",
        menu: "",
        keys: &[(Context::Library, "Ctrl+Up")],
        params: "{}: the previous section's header to the top (mid-section: the current section's first)",
        run: prev_section,
    },
    UiCommand {
        id: "scroll.nextSection",
        label: "Next Section",
        menu: "",
        keys: &[(Context::Library, "Ctrl+Down")],
        params: "{}: the next section's header to the top (after the last: the end of the library)",
        run: next_section,
    },
    UiCommand {
        id: "scroll.top",
        label: "Top of Library",
        menu: "",
        keys: &[(Context::Library, "Ctrl+Home")],
        params: "{}: scroll the light table to the top of the library",
        run: top,
    },
    UiCommand {
        id: "scroll.bottom",
        label: "Bottom of Library",
        menu: "",
        keys: &[(Context::Library, "Ctrl+End")],
        params: "{}: scroll the light table to the bottom of the library",
        run: bottom,
    },
];

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

/// What a scroll key asks for: a page (the view's height less one thumbnail row, at least one row,
/// so a row stays in view) or one of the scroll bar's steps.
enum Kind {
    PageUp,
    PageDown,
    Step(Step),
}

/// Moves the scroll anchor by `kind` on the last frame's layout; answers the new anchor.
fn step(app: &mut App, params: &Value, kind: Kind) -> Result<Value, CmdError> {
    no_params(params)?;
    if app.session().is_none() {
        return Err(CmdError::Disabled("The library is still opening.".into()));
    }
    let nothing = || CmdError::Disabled("There is nothing to scroll.".into());
    let lt = app.panels().map(|p| &p.light_table).ok_or_else(nothing)?;
    let (Some(layout), Some(area)) = (lt.layout(), lt.frame_info().area) else { return Err(nothing()) };
    if layout.is_empty() {
        return Err(nothing());
    }
    let view_h = area.height();
    let page = (f64::from(view_h) - layout.row_pitch()).max(layout.row_pitch());
    let step = match kind {
        Kind::PageUp => Step::By(-page),
        Kind::PageDown => Step::By(page),
        Kind::Step(s) => s,
    };
    let to = scrollbar::target(layout, &app.state().scroll, lt.offset(), view_h, step);
    app.state_mut().scroll = to;
    Ok(json!({ "scroll": to }))
}

fn page_up(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Kind::PageUp)
}

fn page_down(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Kind::PageDown)
}

fn prev_section(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Kind::Step(Step::PrevSection))
}

fn next_section(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Kind::Step(Step::NextSection))
}

fn top(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Kind::Step(Step::Top))
}

fn bottom(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    step(app, params, Kind::Step(Step::Bottom))
}
