//! Keyboard-only use of the window (`ui-layout.md` → Keyboard and screen readers): Tab and
//! Shift+Tab walk the controls in reading order, region by region; a focused control takes its own
//! Enter, Space and arrows; a focused light-table cell leaves the arrows to the keymap and the focus
//! follows the selection. The menu bar's keys (F10, Alt, the arrows in an open menu) are
//! [`crate::panels::menubar::keyboard`]'s.
//!
//! **Reading order.** egui moves the focus in the order widgets were drawn, which is not always the
//! order they are read in (a dialog draws its buttons right to left). Here every control that takes
//! the focus in the last frame (enabled, focusable, on a layer that takes input: under an open
//! dialog only the dialog's) is grouped by the window region it lies in, the regions taken in
//! reading order, and each region's controls in reading order: rows top to bottom (a control
//! starting above the middle of a row's first control is on that row), each row left to right, by
//! the part of it that shows. A control scrolled out of sight is read right after the last one
//! drawn before it that shows (and `ui-kit`'s `register` scrolls it into view as it gains the focus).
//! Controls outside every region (a dialog's, a menu's) come after the regions, in reading order.
//! Tab moves to the next control and wraps; Shift+Tab to the one before. egui's own Tab move is
//! cancelled for the frame, so the focus moves once.

use std::collections::HashSet;

use dalica_ui_kit::registry::{self, Role, WidgetInfo};
use egui::{Context, Event, FocusDirection, Id, Key, Modifiers, Rect};

use crate::app::App;

/// The keys a focused control (or an open menu) takes for itself, pressed with no modifier.
const CONTROL_KEYS: [Key; 6] = [Key::Enter, Key::Space, Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight];
/// What an open menu takes besides: the keys a menu walks with.
const MENU_KEYS: [Key; 5] = [Key::Escape, Key::Home, Key::End, Key::PageUp, Key::PageDown];
/// The most controls ordered for Tab; a frame with more (never seen: the light table draws a
/// screenful) orders the first ones.
const MAX_STOPS: usize = 4096;

fn anchor_key() -> Id {
    Id::new("dalica.a11y.focus-anchor")
}

/// Where the focus was when the frame's keys were read: on a light-table cell, with this anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CellFocus {
    anchor: Option<dalica_core::PhotoId>,
}

/// Before the frame's shortcuts run: Tab and Shift+Tab, the menu bar's keys, and the arrows on a
/// focused cell. Returns the key events held back from the shortcuts for the focused control or
/// the open menu; [`give_back`] returns them once the shortcuts have run, so the widgets see them.
pub fn before_keys(app: &mut App, ctx: &Context) -> Vec<Event> {
    tab(ctx);
    crate::panels::menubar::keyboard(app, ctx);
    // Most frames: no key, or nothing focused; nothing to look up.
    let focused = ctx.memory(|m| m.focused()).filter(|_| pressed(ctx, |_, _| true));
    let widget = focused.and_then(|f| registry::widgets(ctx).into_iter().find(|w| w.egui_id == Some(f)));
    let on_cell = widget.as_ref().is_some_and(is_cell);
    let arrow = pressed(ctx, |k, m| m.is_none() && matches!(k, Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight));
    if on_cell {
        // The keymap moves the selection; egui must not move the focus as well.
        if arrow {
            ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
        }
        ctx.data_mut(|d| d.insert_temp(anchor_key(), Some(CellFocus { anchor: app.state().selection.anchor })));
    } else {
        ctx.data_mut(|d| d.insert_temp::<Option<CellFocus>>(anchor_key(), None));
    }
    let menu = egui::Popup::is_any_open(ctx);
    let control = widget.is_some() && !on_cell;
    if !(menu || control) {
        return Vec::new();
    }
    ctx.input_mut(|i| {
        let (held, rest): (Vec<Event>, Vec<Event>) = std::mem::take(&mut i.events).into_iter().partition(|e| match e {
            Event::Key { key, modifiers, .. } if modifiers.is_none() => CONTROL_KEYS.contains(key) || (menu && MENU_KEYS.contains(key)),
            _ => false,
        });
        i.events = rest;
        held
    })
}

/// Puts back what [`before_keys`] held, for the widgets drawn next.
pub fn give_back(ctx: &Context, held: Vec<Event>) {
    if !held.is_empty() {
        ctx.input_mut(|i| i.events.extend(held));
    }
}

/// At the end of the frame: a focused cell whose selection moved hands the focus to the new
/// anchor's cell.
pub fn after_frame(app: &App, ctx: &Context) {
    let Some(Some(before)) = ctx.data(|d| d.get_temp::<Option<CellFocus>>(anchor_key())) else { return };
    let anchor = app.state().selection.anchor;
    if anchor == before.anchor {
        return;
    }
    let Some(anchor) = anchor else { return };
    let id = match app.state().selection.section {
        Some(section) => crate::panels::light_table::cell::thumb_id(section, anchor),
        None => format!("thumb:{}", anchor.get()),
    };
    if let Some(id) = registry::find(ctx, &id).and_then(|w| w.egui_id) {
        ctx.memory_mut(|m| m.request_focus(id));
    }
}

/// A light-table cell (or the light table itself), whose arrows are the keymap's.
fn is_cell(w: &WidgetInfo) -> bool {
    w.role == Role::Thumbnail || w.id == "lightTable"
}

/// Whether a key matching `which` was pressed this frame.
fn pressed(ctx: &Context, which: impl Fn(Key, Modifiers) -> bool) -> bool {
    ctx.input(|i| i.events.iter().any(|e| matches!(e, Event::Key { key, pressed: true, modifiers, .. } if which(*key, *modifiers))))
}

/// Tab or Shift+Tab: the next or previous control in reading order.
fn tab(ctx: &Context) {
    let forward = pressed(ctx, |k, m| k == Key::Tab && m.is_none());
    let back = pressed(ctx, |k, m| k == Key::Tab && m.shift_only());
    if !(forward || back) {
        return;
    }
    // egui's own move (draw order) is replaced by this one.
    ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
    let stops = order(ctx);
    if stops.is_empty() {
        return;
    }
    let focused = ctx.memory(|m| m.focused());
    let at = focused.and_then(|f| stops.iter().position(|s| *s == f));
    let n = stops.len();
    let next = match (at, forward) {
        (Some(i), true) => (i + 1) % n,
        (Some(i), false) => (i + n - 1) % n,
        (None, true) => 0,
        (None, false) => n - 1,
    };
    if let Some(id) = stops.get(next) {
        ctx.memory_mut(|m| m.request_focus(*id));
    }
}

/// The controls of the last frame that take the focus, in the order Tab walks them.
pub fn order(ctx: &Context) -> Vec<Id> {
    // A control scrolled out of sight (a dialog body's lower rows) is read where it is drawn: right
    // after the last control drawn before it that shows, not by where it lies outside its clip.
    let candidates: Vec<(Id, egui::LayerId, Rect)> = ctx.viewport(|v| {
        let mut out = Vec::new();
        for (layer, list) in v.prev_pass.widgets.layers() {
            let mut shown: Option<Rect> = None;
            for w in list {
                if w.interact_rect.is_positive() && w.interact_rect.is_finite() {
                    shown = Some(w.interact_rect);
                }
                let takes = w.enabled && w.sense.is_focusable() && w.rect.is_positive() && w.rect.is_finite() && w.id != layer.id.with("move");
                if takes && out.len() < MAX_STOPS {
                    let at = if w.interact_rect.is_positive() { w.interact_rect } else { shown.unwrap_or(w.rect) };
                    out.push((w.id, *layer, at));
                }
            }
        }
        out
    });
    let layers: HashSet<egui::LayerId> = candidates.iter().map(|c| c.1).collect();
    let open: HashSet<egui::LayerId> = ctx.memory(|m| layers.into_iter().filter(|l| m.allows_interaction(*l) && m.areas().is_visible(l)).collect());
    let regions = regions(ctx);
    let mut groups: Vec<Vec<(Id, Rect)>> = vec![Vec::new(); regions.len() + 1];
    for (id, layer, rect) in candidates {
        if !open.contains(&layer) {
            continue;
        }
        let group = if layer.order == egui::Order::Background {
            regions.iter().position(|r| r.contains(rect.center())).unwrap_or(regions.len())
        } else {
            regions.len()
        };
        if let Some(g) = groups.get_mut(group) {
            g.push((id, rect));
        }
    }
    groups.into_iter().flat_map(|g| reading_order(g).into_iter().map(|(id, _)| id)).collect()
}

/// The window's regions of the last frame, in reading order.
fn regions(ctx: &Context) -> Vec<Rect> {
    let rects: Vec<((), Rect)> = registry::widgets(ctx).into_iter().filter(|w| w.id.starts_with("region:")).map(|w| ((), rect_of(&w))).collect();
    reading_order(rects).into_iter().map(|(_, r)| r).collect()
}

fn rect_of(w: &WidgetInfo) -> Rect {
    let [x, y, width, height] = w.rect;
    Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height))
}

/// `items` in reading order: rows top to bottom (an item that starts above the middle of the row's
/// first item is on that row), each row left to right.
pub fn reading_order<T>(mut items: Vec<(T, Rect)>) -> Vec<(T, Rect)> {
    items.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()).then(a.1.left().total_cmp(&b.1.left())));
    let mut out = Vec::with_capacity(items.len());
    let mut row: Vec<(T, Rect)> = Vec::new();
    for item in items {
        let starts_row = row.first().is_none_or(|first| item.1.top() >= first.1.center().y);
        if starts_row {
            row.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
            out.append(&mut row);
        }
        row.push(item);
    }
    row.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
    out.append(&mut row);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn reading_order_is_rows_top_to_bottom_each_left_to_right() {
        // A dialog's button row drawn right to left (OK last), a field above it, a checkbox a
        // little lower than the label beside it.
        let items = vec![
            ("apply", r(300.0, 200.0, 50.0, 24.0)),
            ("cancel", r(240.0, 200.0, 50.0, 24.0)),
            ("ok", r(180.0, 202.0, 50.0, 24.0)),
            ("field", r(20.0, 100.0, 200.0, 20.0)),
            ("check", r(240.0, 104.0, 14.0, 14.0)),
            ("below", r(20.0, 140.0, 100.0, 20.0)),
        ];
        let order: Vec<&str> = reading_order(items).into_iter().map(|(n, _)| n).collect();
        assert_eq!(order, ["field", "check", "below", "ok", "cancel", "apply"]);
        assert!(reading_order::<()>(Vec::new()).is_empty());
    }
}
