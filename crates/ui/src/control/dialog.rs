//! `ui.dialog.confirm` and `ui.dialog.cancel` (`control-protocol.md` §6.10): the top in-app dialog
//! answered as its buttons answer it ([`crate::dialogs::answer`], [`crate::dialogs::cancel`]).

use serde::Deserialize;
use serde_json::Value;

use super::host::Input;
use super::{Call, Code, Error, Outcome};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirm {
    button: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

/// The longest button name taken.
const MAX_BUTTON: usize = 128;

fn no_dialog() -> Error {
    Error::new(Code::NotFound, "no dialog is open (ui.inspect → dialog)")
}

/// `ui.dialog.confirm {button?}`: presses the top dialog's default button, or `button`
/// (`dialog.undoSave`, with or without its `button:` prefix); replies what it ran, once a frame has
/// shown the result.
pub(super) fn confirm(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Confirm = call.params(params)?;
    let action = match &p.button {
        None => None,
        Some(b) => {
            let name = b.strip_prefix("button:").unwrap_or(b);
            match name.strip_prefix("dialog.") {
                Some(a) if !a.is_empty() && b.len() <= MAX_BUTTON => Some(a.to_string()),
                _ => {
                    return Err(call.bad(format!(
                        "`button` names a dialog button, `dialog.<action>` (`dialog.ok`), not `{}`",
                        b.chars().take(64).collect::<String>()
                    )));
                }
            }
        }
    };
    if crate::dialogs::top(call.host.app()).is_none() {
        return Err(no_dialog());
    }
    never_automated_button(call.host.app(), action.as_deref())?;
    let saves = crate::dialogs::top(call.host.app()).map(|d| d.kind) == Some(crate::state::DialogKind::CollageClose)
        && matches!(action.as_deref(), None | Some(crate::dialogs::collage_close::SAVE_DRAFT));
    let answer: super::Resume = Box::new(move |host| match crate::dialogs::answer(host.app_mut(), action.as_deref()) {
        Ok(reply) => Outcome::play_then_reply(Input::Frames(1), reply),
        Err(e) => Outcome::from(Error::from(e)),
    });
    // The close question's Save Draft writes into the Collage room's folder: checked first
    // (`control-protocol.md` §5.7).
    if saves {
        let config = call.control.config().clone();
        return Ok(super::vet::collage_first(call.host, &config, answer));
    }
    Ok(answer(call.host))
}

/// Refuses pressing button `action` of the top dialog when it runs a command automation never
/// runs (Remove from Card's Remove Imported Files): `external_denied`, before anything happens. The
/// button refuses by itself too while a request runs (`crate::control::refuse`); this answers the
/// request with the reason.
pub(super) fn never_automated_button(app: &crate::App, action: Option<&str>) -> Result<(), Error> {
    use crate::dialogs::remove_from_card;
    let top = crate::dialogs::top(app).map(|d| d.kind);
    if top == Some(crate::state::DialogKind::RemoveFromCard) && action == Some(remove_from_card::REMOVE_ACTION) {
        dalica_engine::automation::never_automated(remove_from_card::COMMAND)?;
    }
    Ok(())
}

/// `ui.dialog.cancel`: closes the top dialog, changing nothing (its Cancel button, Esc).
pub(super) fn cancel(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let _: NoParams = call.params(params)?;
    if crate::dialogs::top(call.host.app()).is_none() {
        return Err(no_dialog());
    }
    let reply = crate::dialogs::cancel(call.host.app_mut())?;
    Ok(Outcome::play_then_reply(Input::Frames(1), reply))
}
