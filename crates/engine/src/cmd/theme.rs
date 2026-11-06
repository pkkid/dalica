//! The theme commands (M16.4; `design-system.md` §2, `menus-and-shortcuts.md` §1 and §5), after
//! PhotoCraft's `window.theme.<id>` (`crates/ui-egui/src/theme.rs` at 3a896a5e):
//!
//! - `theme.<id>`, one per theme of [`THEMES`] (View ▸ Theme ▸ <label>, a radio row, checked while
//!   it is the theme in effect, [`checked`]): stores `interface.theme` through `options.set`, so
//!   the one settings write is used (written once, atomically, on the I/O worker; a read-only
//!   `settings.json` is refused as `disabled` with the reason, and nothing changes). No undo step
//!   (the setting is the way back); no default keys. Answers `{theme, changed}`.
//! - `theme.next` (no menu row, for scripts): the theme after the one in effect, round the list
//!   (after the last comes Dark); answers as `theme.<id>` does.
//!
//! The commands are made from [`THEMES`] and [`THEME_COMMANDS`] (`settings.rs`), so a theme added
//! there gains its command and menu row here with no other change. The window reads the setting
//! every frame and maps the id to its palette through `ui-kit`'s `ThemeKind` (M16.5).

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;
use crate::settings::{THEME_COMMANDS, THEMES};

/// A command's `run`.
type Run = fn(&mut Session, &Value) -> Result<Value, CmdError>;

/// One `run` per place in [`THEMES`] (a `run` cannot carry its theme); a theme past the last has
/// no command, which a test catches.
macro_rules! runs {
    ($($i:literal)*) => { [$(|s: &mut Session, p: &Value| set_nth(s, p, $i)),*] };
}
const RUNS: [Run; 32] = runs![0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31];

pub fn specs() -> Vec<CommandSpec> {
    let mut specs: Vec<CommandSpec> = THEMES
        .iter()
        .zip(THEME_COMMANDS)
        .zip(RUNS)
        .map(|(((_, label), (id, menu)), run)| CommandSpec {
            id,
            label,
            menu,
            keys: &[],
            params: r#"{}: makes this the window's theme (interface.theme in settings.json); no undo step; answers {"theme": id, "changed": bool}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run,
        })
        .collect();
    specs.push(CommandSpec {
        id: "theme.next",
        label: "Next Theme",
        menu: "",
        keys: &[],
        params: r#"{}: makes the theme after the one in effect the window's theme, round the list (View ▸ Theme ▸'s order); no undo step; answers {"theme": id, "changed": bool}"#,
        effect: Effect::Library,
        history: History::None,
        enabled: always,
        run: next,
    });
    specs
}

/// Whether the row of command `id` is checked: `Some(true)` for the `theme.<id>` of the theme in
/// effect, `Some(false)` for the other themes' commands, `None` for any other command.
pub fn checked(s: &Session, id: &str) -> Option<bool> {
    let place = THEME_COMMANDS.iter().position(|(command, _)| *command == id)?;
    let theme = THEMES.get(place)?.0;
    Some(s.settings().interface().theme == theme)
}

/// The command that makes `theme` the window's theme (`theme.<id>`); `None` for an id this version
/// does not know.
pub fn command(theme: &str) -> Option<&'static str> {
    let place = THEMES.iter().position(|(id, _)| *id == theme)?;
    THEME_COMMANDS.get(place).map(|(command, _)| *command)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

fn set_nth(s: &mut Session, p: &Value, n: usize) -> Result<Value, CmdError> {
    let NoParams {} = params(p)?;
    let (theme, _) = THEMES.get(n).ok_or_else(|| CmdError::Internal(format!("no theme {n}")))?;
    store(s, theme)
}

fn next(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let NoParams {} = params(p)?;
    let now = s.settings().interface().theme;
    let place = THEMES.iter().position(|(id, _)| *id == now).unwrap_or(0);
    let (theme, _) =
        THEMES.get(place.saturating_add(1) % THEMES.len().max(1)).or(THEMES.first()).ok_or_else(|| CmdError::Internal("no themes".into()))?;
    store(s, theme)
}

/// Stores `theme` as `interface.theme` through `options.set`'s one write.
fn store(s: &mut Session, theme: &'static str) -> Result<Value, CmdError> {
    let run = s.spec("options.set").map(|c| c.run).ok_or_else(|| CmdError::Internal("options.set is not registered".into()))?;
    let answer = run(s, &json!({ "values": { "interface.theme": theme } }))?;
    let changed = answer.get("changed").and_then(Value::as_array).is_some_and(|keys| !keys.is_empty());
    Ok(json!({ "theme": theme, "changed": changed }))
}
