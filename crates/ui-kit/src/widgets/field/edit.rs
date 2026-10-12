//! How every text field edits (`design-system.md` §6): right-click for Cut, Copy, Paste and Select
//! All; Esc gives back the text from before the edit; Return or leaving the field ends the edit,
//! once; optionally everything is selected when the field takes the focus, for fields that are
//! retyped whole.
//!
//! Adapted from LightCraft `crates/ui-egui/src/text_field.rs` at revision 76b53778
//! (<https://github.com/storytold/lightcraft>, MIT OR Apache-2.0; `NOTICE`). Changes for Dalica:
//! the field keeps `ui-kit`'s own frame and geometry (the caller paints it, [`super`]); its egui id
//! is `Id::new(<automation id>)`; menu items register as `<field id>/cut`, `/copy`, `/paste` and
//! `/selectAll` (role `menuItem`) through [`crate::widgets::register`]; no translations, browser
//! paths or multi-line mode; never a panic.
//!
//! The caller owns the text and learns how an edit ended from [`Ending`]; what an ending means
//! (save, apply, close) is the caller's. Each field's automation id is unique on screen: its state
//! is keyed by it. The keyboard's Ctrl/Cmd+X, C, V, A and undo are egui's own.

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{Event, Id, Key, PointerButton, Popup, PopupCloseBehavior, Rect, Response, TextEdit, Ui, ViewportCommand};

use crate::Tokens;
use crate::fonts;
use crate::registry::Role;
use crate::widgets::{Target, register};

/// How an edit ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Return: the text is meant.
    Return,
    /// The focus went elsewhere (a click outside, Tab): the text is kept.
    Away,
    /// Esc: the text is back to what it was before the edit.
    Escape,
}

/// What a field's edit core did this pass.
pub(crate) struct Edited {
    pub response: Response,
    /// Set on the pass an edit ends.
    pub ending: Option<Ending>,
    /// An edit is under way: the field has the focus, or its menu does.
    pub editing: bool,
}

/// What the edit core needs to know about a field.
pub(crate) struct Spec<'a> {
    /// The automation id, which also keys the field's state.
    pub id: &'a str,
    pub hint: &'a str,
    /// Where the text goes (inside the frame the caller paints).
    pub rect: Rect,
    pub enabled: bool,
    pub select_on_focus: bool,
}

/// The egui id of the field registered as `id`: its focus and its `TextEditState`.
pub(crate) fn egui_id(id: &str) -> Id {
    Id::new(id)
}

fn memo_id(id: &str) -> Id {
    egui_id(id).with("memo")
}

/// What a field remembers between passes.
#[derive(Clone, Default)]
struct Memo {
    /// The text when the edit started, for Esc.
    before: Option<String>,
    /// The context menu was open at the end of the pass.
    menu_open: bool,
    /// The menu's choice, done next pass.
    action: Option<Action>,
    /// The focus left for the menu: the edit ends if the menu closes without a choice.
    away: bool,
    /// The field took the focus with select-on-focus under a press: select all on its release,
    /// unless it dragged a selection.
    select_on_release: bool,
    /// The pass (egui's cumulative pass number) the field was last drawn in.
    pass: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// Only take the focus back (Esc closed the menu).
    Refocus,
}

/// The text edit of a field, with the behaviour of the module docs.
pub(crate) fn edit(ui: &mut Ui, t: &Tokens, spec: Spec<'_>, text: &mut String) -> Edited {
    let id = egui_id(spec.id);
    let key = memo_id(spec.id);
    let mut memo: Memo = ui.data_mut(|d| d.get_temp(key)).unwrap_or_default();
    // A field not drawn last pass (its dialog or panel closed) starts afresh: an edit, a menu or a
    // choice from it that was under way is gone with it.
    let pass = ui.ctx().cumulative_pass_nr();
    if memo.pass.checked_add(1) != Some(pass) {
        memo = Memo::default();
    }
    memo.pass = pass;
    // A choice from the menu last pass: the field takes the focus back and does it, with egui's own
    // handling of the clipboard events.
    if let Some(action) = memo.action.take() {
        ui.memory_mut(|m| m.request_focus(id));
        match action {
            Action::Cut => ui.input_mut(|i| i.events.push(Event::Cut)),
            Action::Copy => ui.input_mut(|i| i.events.push(Event::Copy)),
            // Only the host can read the clipboard: it answers with a paste event.
            Action::Paste => ui.ctx().send_viewport_cmd(ViewportCommand::RequestPaste),
            Action::SelectAll => select_all(ui.ctx(), id, text),
            Action::Refocus => {}
        }
    }
    let hint = egui::RichText::new(spec.hint).color(t.text_dim).font(fonts::regular(t.body));
    let edit = TextEdit::singleline(text)
        .id(id)
        .frame(egui::Frame::NONE)
        .hint_text(hint)
        .font(fonts::regular(t.body))
        .text_color(if spec.enabled { t.text } else { t.text_disabled })
        .margin(egui::Margin::ZERO)
        .desired_width(spec.rect.width())
        .vertical_align(egui::Align::Center);
    let (escape, enter) = ui.input(|i| (i.key_pressed(Key::Escape), i.key_pressed(Key::Enter)));
    // A right-click places the cursor as a left one does: the selection the menu acts on would be
    // gone.
    let right = ui.input(|i| i.pointer.button_down(PointerButton::Secondary) || i.pointer.button_released(PointerButton::Secondary));
    let selection = TextEditState::load(ui.ctx(), id).and_then(|s| s.cursor.char_range());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(spec.rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let mut response = child.add_enabled(spec.enabled, edit);
    // The menu may take the focus on a primary-button release, and egui collapses a selection when
    // the focus leaves: keep the range the menu acts on.
    if right && response.contains_pointer() || memo.menu_open && !ui.memory(|m| m.has_focus(id)) {
        let mut state = TextEditState::load(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(selection);
        state.store(ui.ctx(), id);
    }
    // egui's own focus: `Response::has_focus` is also false while the window is in the background
    // (another app in front), which neither ends nor restarts an edit.
    let focused = ui.memory(|m| m.has_focus(id));

    // An edit not under way (none, or one cut short when the field went away) is forgotten.
    let in_menu = memo.away || memo.menu_open || memo.action.is_some();
    if !focused && !response.lost_focus() && !in_menu {
        memo.before = None;
    }
    // The start of an edit (not the focus coming back from the menu).
    let pressing = ui.input(|i| i.pointer.any_down());
    if focused && memo.before.is_none() {
        memo.before = Some(text.clone());
        if spec.select_on_focus {
            // A press that took the focus may go on to drag a selection: wait for its release.
            if pressing {
                memo.select_on_release = true;
            } else {
                select_all(ui.ctx(), id, text);
            }
        }
    }
    if memo.select_on_release && !pressing {
        memo.select_on_release = false;
        // A plain click selects everything; a drag keeps what it selected.
        if focused && !has_selection(ui.ctx(), id) {
            select_all(ui.ctx(), id, text);
        }
    }
    let menu_was_open = memo.menu_open;
    // A click on a greyed-out item does nothing, as in a native menu: only a choice or a click
    // outside closes it.
    let mut action = None;
    memo.menu_open = spec.enabled
        && Popup::context_menu(&response)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| menu(ui, spec.id, id, text, &mut action))
            .is_some();
    if action.is_some() {
        memo.action = action;
    }

    let mut ending = None;
    let popup = Popup::default_response_id(&response);
    // egui reports a lost focus on two passes: only an edit under way ends.
    if response.lost_focus() && memo.before.is_some() {
        if escape && menu_was_open {
            // Esc closes only the menu, as in native menus: the field takes the focus back.
            memo.action = Some(Action::Refocus);
        } else if escape {
            if let Some(before) = memo.before.take()
                && *text != before
            {
                *text = before;
                response.mark_changed();
            }
            ending = Some(Ending::Escape);
        } else if enter {
            ending = Some(Ending::Return);
            Popup::close_id(ui.ctx(), popup);
        } else if menu_was_open || memo.menu_open || memo.action.is_some() {
            // The click went to the menu: the edit goes on once the menu is done with.
            memo.away = true;
        } else {
            ending = Some(Ending::Away);
        }
    } else if memo.away && escape && memo.action.is_none() {
        // Esc with the focus in the menu (Tab took it there) closes only the menu.
        memo.action = Some(Action::Refocus);
        memo.away = false;
    } else if memo.away && !memo.menu_open && memo.action.is_none() && !focused {
        // The menu closed without a choice: the click outside it left the field.
        ending = Some(Ending::Away);
    }
    if focused {
        memo.away = false;
    }
    if ending.is_some() {
        memo = Memo { pass, ..Memo::default() };
    }
    let editing = ending.is_none() && (focused || memo.away || memo.menu_open || memo.action.is_some());
    ui.data_mut(|d| d.insert_temp(key, memo));
    Edited { response, ending, editing }
}

fn select_all(ctx: &egui::Context, id: Id, text: &str) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    let end = CCursor::new(text.chars().count());
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
    state.store(ctx, id);
}

/// Some text is selected in the field.
fn has_selection(ctx: &egui::Context, id: Id) -> bool {
    TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()).is_some_and(|r| !r.is_empty())
}

/// The field's context menu: Cut, Copy, Paste, Select All, with the keyboard's shortcuts beside
/// them (macOS writes `Ctrl` as `Cmd`, as the menu bar does).
fn menu(ui: &mut Ui, widget: &str, id: Id, text: &str, action: &mut Option<Action>) {
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    let selected = has_selection(ui.ctx(), id);
    let items = [
        (Action::Cut, "cut", "Cut", "X", selected),
        (Action::Copy, "copy", "Copy", "C", selected),
        (Action::Paste, "paste", "Paste", "V", true),
        (Action::SelectAll, "selectAll", "Select All", "A", !text.is_empty()),
    ];
    for (a, key, label, letter, enabled) in items {
        if a == Action::SelectAll {
            ui.separator();
        }
        let shortcut = format!("{}+{letter}", if mac { "Cmd" } else { "Ctrl" });
        let r = ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut));
        register(ui.ctx(), &format!("{widget}/{key}"), Role::MenuItem, label, Target::Widget(&r), false);
        if r.clicked() {
            *action = Some(a);
            ui.close();
        }
    }
}
