//! Text fields and the autocomplete list (`design-system.md` §6), driven headlessly as a user
//! would: clicks, keys and typed text through egui's input, the clipboard answered as the host
//! does (copied text taken from the output, a paste request answered with a paste event).
//!
//! The behaviour tests of the text field are ported from LightCraft
//! `crates/ui-egui/src/text_field.rs` at revision 76b53778 (MIT OR Apache-2.0; `NOTICE`), with
//! Dalica's ids (`<field>/cut` …). The gallery renders every state of the fields and the list with
//! the CPU rasterizer: a sheet of states (`fingerprints/fields-states.json`) and a window
//! with a field at its bottom-right corner, its list kept inside the window
//! (`fingerprints/fields-window-1400x900.json`). The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{check, out_dir};
use dalica_ui_kit::widgets::{Ending, TextField, autocomplete, focus_field, search_field, text_field};
use dalica_ui_kit::{Image, Rasterizer, Role, Tokens, WidgetInfo, fonts, registry};
use egui::text_edit::TextEditState;
use egui::{Align2, Context, Event, Key, Modifiers, OutputCommand, PointerButton, Pos2, Rect, UiBuilder, ViewportCommand, pos2, vec2};

const FIELD: &str = "field:city";
const CLEAR: &str = "button:city.clear";

/// What the rig draws.
#[derive(Clone)]
enum Kind {
    Text,
    Search,
    /// A text field with an autocomplete list of these options.
    Auto(Vec<String>),
}

/// A field alone in a window, with blank room around it to click away to.
struct Rig {
    ctx: Context,
    t: Tokens,
    kind: Kind,
    size: (u32, u32),
    /// The field's top-left corner and width.
    at: Pos2,
    width: f32,
    text: String,
    select_on_focus: bool,
    /// The field is on screen (a dialog that holds it may close).
    shown: bool,
    /// The app's window has the system's focus (the user may switch to another app).
    window_focused: bool,
    /// Painted on the kept frame (gallery tiles).
    caption: Option<&'static str>,
    /// The field said an edit is under way, last pass.
    editing: bool,
    endings: Vec<Ending>,
    /// The autocomplete's picks, and whether its list was open last pass.
    picks: Vec<usize>,
    open: bool,
    /// The system clipboard, and a paste the app asked for (answered next pass).
    clipboard: String,
    paste: bool,
    time: f64,
    widgets: Vec<WidgetInfo>,
    /// Takes every pass's textures, so a pass can be painted at any time.
    raster: Rasterizer,
}

impl Rig {
    fn new(text: &str) -> Self {
        Rig::with(Kind::Text, text, (400, 300), pos2(8.0, 20.0))
    }

    fn with(kind: Kind, text: &str, size: (u32, u32), at: Pos2) -> Self {
        let t = Tokens::default();
        let ctx = Context::default();
        t.apply(&ctx);
        let mut rig = Rig {
            ctx,
            t,
            kind,
            size,
            at,
            width: 200.0,
            text: text.to_owned(),
            select_on_focus: false,
            shown: true,
            window_focused: true,
            caption: None,
            editing: false,
            endings: vec![],
            picks: vec![],
            open: false,
            clipboard: String::new(),
            paste: false,
            time: 0.0,
            widgets: vec![],
            raster: Rasterizer::new(),
        };
        rig.settle();
        rig
    }

    fn auto(options: &[&str]) -> Self {
        Rig::with(Kind::Auto(options.iter().map(|s| s.to_string()).collect()), "", (400, 300), pos2(8.0, 20.0))
    }

    /// One pass; returns its output for painting.
    fn pass(&mut self, events: Vec<Event>) -> egui::FullOutput {
        self.time += 1.0 / 60.0;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.time);
        raw.focused = self.window_focused;
        if std::mem::take(&mut self.paste) {
            raw.events.push(Event::Paste(self.clipboard.clone()));
        }
        raw.events.extend(events);
        let Rig { ctx, t, kind, at, width, text, select_on_focus, shown, caption, editing, endings, picks, open, .. } = self;
        let out = ctx.run_ui(raw, |ui| {
            if let Some(c) = caption {
                ui.painter().text(pos2(6.0, 3.0), Align2::LEFT_TOP, *c, fonts::regular(t.small), t.text_dim);
            }
            if !*shown {
                return;
            }
            let rect = Rect::from_min_size(*at, vec2(*width, 30.0));
            ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                let (field, pick, is_open) = match kind {
                    Kind::Text => (TextField::new(FIELD, "City", *width).select_on_focus(*select_on_focus).show(ui, t, text), None, false),
                    Kind::Search => (search_field(ui, t, FIELD, CLEAR, text, "Search", *width).into(), None, false),
                    Kind::Auto(options) => {
                        let out = autocomplete(ui, t, FIELD, text, options, |ui, text| text_field(ui, t, FIELD, text, "Add a tag", *width));
                        (out.field, out.picked, out.open)
                    }
                };
                endings.extend(field.ending);
                *editing = field.editing;
                picks.extend(pick);
                *open = is_open;
            });
        });
        for c in &out.platform_output.commands {
            if let OutputCommand::CopyText(s) = c {
                self.clipboard = s.clone();
            }
        }
        if out.viewport_output.values().any(|v| v.commands.iter().any(|c| matches!(c, ViewportCommand::RequestPaste))) {
            self.paste = true;
        }
        self.widgets = registry::widgets(&self.ctx);
        out
    }

    fn frame(&mut self, events: Vec<Event>) {
        let mut delta = self.pass(events).textures_delta;
        self.raster.set_textures(&mut delta);
        self.raster.free_textures(&mut delta);
    }

    /// One more pass, painted.
    fn render(&mut self) -> Image {
        let out = self.pass(vec![]);
        dalica_ui_kit::paint(&self.ctx, &mut self.raster, out, self.size.0, self.size.1, self.t.chrome).unwrap()
    }

    /// A few passes, for menus to open and requests to come back.
    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }

    fn widget(&self, id: &str) -> Option<&WidgetInfo> {
        self.widgets.iter().rev().find(|w| w.id == id)
    }

    fn rect(&self, id: &str) -> Option<Rect> {
        self.widget(id).map(|w| Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3])))
    }

    fn click_at(&mut self, at: Pos2, button: PointerButton) {
        // Far apart in time from the last click: two clicks never make a double-click.
        self.time += 1.0;
        let press = |pressed| Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(at), press(true)]);
        self.frame(vec![press(false)]);
        self.settle();
    }

    fn click(&mut self, id: &str, button: PointerButton) {
        let r = self.rect(id).unwrap_or_else(|| panic!("no {id} on screen: {:?}", self.ids()));
        self.click_at(r.center(), button);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        self.frame(vec![ev(true), ev(false)]);
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_string())]);
        self.settle();
    }

    fn focused(&self) -> bool {
        self.ctx.memory(|m| m.focused()).is_some()
    }

    fn ids(&self) -> Vec<&str> {
        self.widgets.iter().map(|w| w.id.as_str()).collect()
    }

    /// The autocomplete rows on screen: (option index, highlighted).
    fn rows(&self) -> Vec<(usize, bool)> {
        let prefix = format!("{FIELD}/option/");
        self.widgets.iter().filter_map(|w| w.id.strip_prefix(&prefix).and_then(|i| i.parse().ok()).map(|i| (i, w.active))).collect()
    }

    /// Click into the field and select all its text with Ctrl/Cmd+A.
    fn select_all_by_keys(&mut self) {
        self.click(FIELD, PointerButton::Primary);
        self.key(Key::A, Modifiers::COMMAND);
    }

    /// Right-click the field, then pick `item` from its menu.
    fn menu(&mut self, item: &str) {
        self.click(FIELD, PointerButton::Secondary);
        self.click(&format!("{FIELD}/{item}"), PointerButton::Primary);
    }
}

// ---- The text field (ported from LightCraft) ----

/// Right-click offers Cut, Copy, Paste and Select All, registered as menu items.
#[test]
fn right_click_offers_cut_copy_paste_and_select_all() {
    let mut rig = Rig::new("Lisbon");
    rig.click(FIELD, PointerButton::Secondary);
    for (item, label) in [("cut", "Cut"), ("copy", "Copy"), ("paste", "Paste"), ("selectAll", "Select All")] {
        let w = rig.widget(&format!("{FIELD}/{item}")).unwrap_or_else(|| panic!("{item}: {:?}", rig.ids()));
        assert_eq!((w.role, w.label.as_str()), (Role::MenuItem, label));
        assert!(registry::is_valid_id(&w.id));
    }
    assert_eq!(registry::unregistered(&rig.ctx), [], "every item has an id");
}

/// Copy in the menu puts the selected text on the clipboard and leaves the field as it was, still
/// being edited.
#[test]
fn copy_in_the_menu_copies_the_selection() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.menu("copy");
    assert_eq!(rig.clipboard, "Lisbon");
    assert_eq!(rig.text, "Lisbon");
    assert!(rig.focused(), "still editing");
    assert_eq!(rig.endings, vec![], "using the menu is not leaving the field");
}

/// Cut in the menu puts the selected text on the clipboard and takes it out of the field.
#[test]
fn cut_in_the_menu_moves_the_selection_to_the_clipboard() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.menu("cut");
    assert_eq!(rig.clipboard, "Lisbon");
    assert_eq!(rig.text, "");
}

/// Paste in the menu puts the clipboard's text in place of the selection.
#[test]
fn paste_in_the_menu_replaces_the_selection() {
    let mut rig = Rig::new("Lisbon");
    rig.clipboard = "Porto".into();
    rig.select_all_by_keys();
    rig.menu("paste");
    assert_eq!(rig.text, "Porto");
}

/// The menu acts on a part of the text: Copy takes only what is selected, Cut takes only it out.
#[test]
fn the_menu_acts_on_a_partial_selection() {
    let mut rig = Rig::new("Lisbon Portugal");
    rig.click(FIELD, PointerButton::Primary);
    let id = egui::Id::new(FIELD);
    let mut state = TextEditState::load(&rig.ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(7), egui::text::CCursor::new(15))));
    state.store(&rig.ctx, id);
    rig.settle();
    rig.menu("copy");
    assert_eq!(rig.clipboard, "Portugal");
    rig.menu("cut");
    assert_eq!((rig.clipboard.as_str(), rig.text.as_str()), ("Portugal", "Lisbon "));
}

/// Select All in the menu selects the whole text: what is typed next replaces it.
#[test]
fn select_all_in_the_menu_selects_the_whole_text() {
    let mut rig = Rig::new("Lisbon");
    rig.click(FIELD, PointerButton::Primary);
    rig.menu("selectAll");
    rig.type_text("Kyoto");
    assert_eq!(rig.text, "Kyoto");
}

#[test]
fn clicking_back_into_field_dismisses_its_menu_and_places_a_new_caret() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.click(FIELD, PointerButton::Secondary);
    let rect = rig.rect(FIELD).unwrap();
    rig.click_at(pos2(rect.left() + 1.0, rect.center().y), PointerButton::Primary);
    let range = TextEditState::load(&rig.ctx, egui::Id::new(FIELD)).unwrap().cursor.char_range().unwrap();
    assert!(range.is_empty(), "a click back in the field places a caret: {range:?}");
    rig.type_text("X");
    assert_eq!(rig.text, "XLisbon");
    assert!(rig.endings.is_empty(), "dismissing into the field keeps the edit open");
}

/// With nothing selected, Cut and Copy are greyed out: clicking one does nothing, as in a native
/// menu, and neither closes the menu nor ends the edit.
#[test]
fn greyed_out_items_do_nothing() {
    let mut rig = Rig::new("Lisbon");
    rig.click(FIELD, PointerButton::Primary);
    rig.click(FIELD, PointerButton::Secondary);
    for item in ["cut", "copy"] {
        let id = format!("{FIELD}/{item}");
        assert_eq!(rig.widget(&id).map(|w| w.enabled), Some(false), "{item} is greyed out");
        rig.click(&id, PointerButton::Primary);
        assert!(rig.rect(&id).is_some(), "{item}: the menu is still open");
    }
    assert_eq!((rig.text.as_str(), rig.clipboard.as_str()), ("Lisbon", ""));
    assert_eq!(rig.endings, vec![], "still editing");
    rig.menu("selectAll");
    rig.type_text("Kyoto");
    assert_eq!(rig.text, "Kyoto", "and the edit goes on");
}

/// Esc gives back the text from before the edit and says the edit was cancelled.
#[test]
fn escape_gives_back_the_text_from_before_the_edit() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    assert_eq!(rig.text, "Rome");
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!(rig.text, "Lisbon");
    assert_eq!(rig.endings, vec![Ending::Escape]);
    assert!(!rig.editing);
}

/// Return ends the edit and keeps the text, once.
#[test]
fn return_ends_the_edit_once() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    rig.key(Key::Enter, Modifiers::NONE);
    rig.settle();
    assert_eq!(rig.text, "Rome");
    assert_eq!(rig.endings, vec![Ending::Return], "one ending, though egui reports the lost focus on two passes");
}

/// Return with the menu open ends the edit as Return does, and closes the menu.
#[test]
fn return_with_the_menu_open_ends_the_edit() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    rig.click(FIELD, PointerButton::Secondary);
    rig.key(Key::Enter, Modifiers::NONE);
    assert_eq!(rig.endings, vec![Ending::Return]);
    assert_eq!(rig.text, "Rome");
    assert!(rig.rect(&format!("{FIELD}/cut")).is_none(), "the menu closed");
}

/// Esc with the menu open closes only the menu, as in native menus: the edit goes on, and a second
/// Esc gives back the text.
#[test]
fn escape_with_the_menu_open_closes_only_the_menu() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    rig.click(FIELD, PointerButton::Secondary);
    rig.key(Key::Escape, Modifiers::NONE);
    assert!(rig.rect(&format!("{FIELD}/cut")).is_none(), "the menu closed");
    assert_eq!((rig.text.as_str(), rig.endings.clone()), ("Rome", vec![]), "the edit goes on");
    assert!(rig.focused());
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!((rig.text.as_str(), rig.endings.clone()), ("Lisbon", vec![Ending::Escape]));
}

/// Esc after Tab took the focus into the open menu also closes only the menu.
#[test]
fn escape_from_inside_the_menu_closes_only_the_menu() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    rig.click(FIELD, PointerButton::Secondary);
    rig.key(Key::Tab, Modifiers::NONE);
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!((rig.text.as_str(), rig.endings.clone()), ("Rome", vec![]), "the edit goes on");
    assert!(rig.focused());
}

/// Clicking elsewhere ends the edit and keeps the text, once.
#[test]
fn clicking_away_ends_the_edit_once() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    rig.click_at(pos2(300.0, 250.0), PointerButton::Primary);
    rig.click_at(pos2(310.0, 260.0), PointerButton::Primary);
    assert_eq!(rig.text, "Rome");
    assert_eq!(rig.endings, vec![Ending::Away]);
}

/// Opening the menu and closing it without a choice (a click outside) leaves the field.
#[test]
fn dismissing_the_menu_leaves_the_field() {
    let mut rig = Rig::new("Lisbon");
    rig.click(FIELD, PointerButton::Primary);
    rig.click(FIELD, PointerButton::Secondary);
    rig.click_at(pos2(300.0, 250.0), PointerButton::Primary);
    assert!(!rig.focused());
    assert_eq!(rig.endings, vec![Ending::Away]);
}

/// A field that selects on focus: clicking into it selects everything, so typing replaces it.
#[test]
fn a_field_that_selects_on_focus_is_retyped_whole() {
    let mut rig = Rig::new("Lisbon");
    rig.select_on_focus = true;
    rig.click(FIELD, PointerButton::Primary);
    rig.type_text("Kyoto");
    assert_eq!(rig.text, "Kyoto");
}

/// Focus given from code (an inline editor opening) selects everything too, so the first key
/// typed replaces the text.
#[test]
fn focus_from_code_selects_all_in_a_field_that_selects_on_focus() {
    let mut rig = Rig::new("Lisbon");
    rig.select_on_focus = true;
    focus_field(&rig.ctx, FIELD);
    rig.settle();
    assert!(rig.editing);
    rig.type_text("Kyoto");
    assert_eq!(rig.text, "Kyoto");
}

/// Esc after using the menu still gives back the text from before the whole edit.
#[test]
fn escape_after_the_menu_gives_back_the_original_text() {
    let mut rig = Rig::new("Lisbon");
    rig.clipboard = "Porto".into();
    rig.select_all_by_keys();
    rig.menu("paste");
    assert_eq!(rig.text, "Porto");
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!(rig.text, "Lisbon");
}

/// A field that goes away in the middle of an edit (its dialog closed) starts afresh when it comes
/// back: Esc then gives back the text it came back with.
#[test]
fn an_edit_cut_short_is_forgotten() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    rig.shown = false;
    rig.settle();
    rig.text = "Porto".into();
    rig.shown = true;
    rig.settle();
    rig.select_all_by_keys();
    rig.type_text("Kyoto");
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!(rig.text, "Porto");
}

/// Switching to another app and back in the middle of an edit carries on with it: the field is
/// still being edited meanwhile, and Esc still gives back the text from before.
#[test]
fn switching_apps_carries_on_with_the_edit() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    rig.window_focused = false;
    rig.settle();
    assert!(rig.editing, "still being edited while the app is in the background");
    assert_eq!(rig.endings, vec![]);
    rig.window_focused = true;
    rig.settle();
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!((rig.text.as_str(), rig.endings.clone()), ("Lisbon", vec![Ending::Escape]));
}

/// A field that goes away while its menu is open starts afresh too when it comes back.
#[test]
fn a_field_gone_with_its_menu_open_starts_afresh() {
    let mut rig = Rig::new("Lisbon");
    rig.select_all_by_keys();
    rig.type_text("Rome");
    rig.click(FIELD, PointerButton::Secondary);
    // A click on the greyed-out Cut takes the focus into the menu.
    rig.click(&format!("{FIELD}/cut"), PointerButton::Primary);
    rig.shown = false;
    rig.settle();
    rig.text = "Porto".into();
    rig.shown = true;
    rig.settle();
    assert!(!rig.editing, "no edit under way");
    assert_eq!(rig.endings, vec![], "and none ended: it was cut short");
    rig.select_all_by_keys();
    rig.type_text("Kyoto");
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!(rig.text, "Porto");
}

/// A field that selects on focus still lets a drag into it select part of the text, as a browser's
/// address bar does: only a plain click selects everything.
#[test]
fn a_drag_into_a_field_that_selects_on_focus_selects_what_it_covers() {
    let mut rig = Rig::new("Lisbon Portugal Europe Atlantic Ocean");
    rig.select_on_focus = true;
    let r = rig.rect(FIELD).expect("the field");
    // Within the field (Dalica's text area is inset in its frame, so a drag past the edge would
    // scroll the text): from near the start to the middle.
    let (from, to) = (pos2(r.left() + 30.0, r.center().y), r.center());
    let press = |at, pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    rig.frame(vec![Event::PointerMoved(from), press(from, true)]);
    for i in 1..=5 {
        rig.frame(vec![Event::PointerMoved(from + (to - from) * (i as f32 / 5.0))]);
    }
    rig.frame(vec![press(to, false)]);
    rig.settle();
    rig.type_text("X");
    assert!(rig.text.starts_with("Lis") && rig.text.contains('X') && rig.text.ends_with("Ocean"), "{:?}", rig.text);
}

// ---- The search field edits the same way ----

/// Esc in the search field gives back the text from before the edit (the filter runs again), and
/// its menu is there too.
#[test]
fn the_search_field_restores_on_escape_and_has_the_menu() {
    let mut rig = Rig::with(Kind::Search, "beach", (400, 300), pos2(8.0, 20.0));
    rig.select_all_by_keys();
    rig.type_text("sun");
    assert_eq!(rig.text, "sun");
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!((rig.text.as_str(), rig.endings.clone()), ("beach", vec![Ending::Escape]));
    rig.click(FIELD, PointerButton::Secondary);
    assert!(rig.widget(&format!("{FIELD}/paste")).is_some(), "{:?}", rig.ids());
    // Clearing keeps the focus in the field, and the edit goes on.
    rig.key(Key::Escape, Modifiers::NONE);
    rig.click(CLEAR, PointerButton::Primary);
    assert!(rig.text.is_empty() && rig.focused() && rig.editing);
}

// ---- The autocomplete list ----

const TAGS: [&str; 6] = ["Summer", "Summer trip", "Sunset", "Beach", "Lisbon summer", "Winter"];

/// Typing filters the list: options holding the text, those starting with it first; a text that
/// matches nothing shows no list, and an empty one neither.
#[test]
fn the_autocomplete_filters_as_you_type() {
    let mut rig = Rig::auto(&TAGS);
    assert!(rig.rows().is_empty() && !rig.open, "no list before typing");
    rig.click(FIELD, PointerButton::Primary);
    assert!(rig.rows().is_empty(), "no list for an empty field");
    rig.type_text("su");
    assert_eq!(rig.rows(), vec![(0, false), (1, false), (2, false), (4, false)]);
    assert!(rig.open);
    let row = rig.widget(&format!("{FIELD}/option/4")).unwrap();
    assert_eq!((row.role, row.label.as_str()), (Role::Suggestion, "Lisbon summer"));
    rig.type_text("m");
    assert_eq!(rig.rows().iter().map(|r| r.0).collect::<Vec<_>>(), vec![0, 1, 4]);
    rig.type_text("x");
    assert!(rig.rows().is_empty() && !rig.open, "nothing matches");
    assert_eq!(registry::unregistered(&rig.ctx), []);
}

/// ↓ and ↑ move the highlight (↑ from the first row goes back to the text typed); Enter picks the
/// highlighted option: its text goes into the field and the edit ends once, with Return.
#[test]
fn the_autocomplete_picks_by_keys() {
    let mut rig = Rig::auto(&TAGS);
    rig.click(FIELD, PointerButton::Primary);
    rig.type_text("su");
    rig.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(rig.rows()[0], (0, true));
    rig.key(Key::ArrowDown, Modifiers::NONE);
    rig.key(Key::ArrowDown, Modifiers::NONE);
    rig.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(rig.rows().iter().filter(|r| r.1).collect::<Vec<_>>(), vec![&(1, true)]);
    assert!(rig.focused() && rig.text == "su", "the arrows neither move the focus nor the text");
    rig.key(Key::Enter, Modifiers::NONE);
    rig.settle();
    assert_eq!(rig.text, "Summer trip");
    assert_eq!(rig.picks, vec![1]);
    assert_eq!(rig.endings, vec![Ending::Return], "one commit, of the picked text");
    assert!(rig.rows().is_empty() && !rig.open, "the list closed");
}

/// With nothing highlighted, Enter commits the text as typed (a new tag), picking nothing.
#[test]
fn enter_without_a_highlight_commits_the_typed_text() {
    let mut rig = Rig::auto(&TAGS);
    rig.click(FIELD, PointerButton::Primary);
    rig.type_text("su");
    rig.key(Key::ArrowDown, Modifiers::NONE);
    rig.key(Key::ArrowUp, Modifiers::NONE);
    assert!(rig.rows().iter().all(|r| !r.1), "↑ from the first row: nothing highlighted");
    rig.key(Key::Enter, Modifiers::NONE);
    assert_eq!((rig.text.as_str(), rig.picks.clone(), rig.endings.clone()), ("su", vec![], vec![Ending::Return]));
}

/// A click on a row picks it: its text goes into the field and the edit ends once.
#[test]
fn the_autocomplete_picks_by_click() {
    let mut rig = Rig::auto(&TAGS);
    rig.click(FIELD, PointerButton::Primary);
    rig.type_text("su");
    let hover = rig.rect(&format!("{FIELD}/option/2")).unwrap().center();
    rig.frame(vec![Event::PointerMoved(hover)]);
    rig.click(&format!("{FIELD}/option/2"), PointerButton::Primary);
    rig.settle();
    assert_eq!(rig.text, "Sunset");
    assert_eq!(rig.picks, vec![2]);
    assert_eq!(rig.endings, vec![Ending::Away], "one commit, of the picked text");
    assert!(rig.rows().is_empty());
}

/// Esc closes the list and keeps the text and the edit; typing opens it again; a second Esc gives
/// back the field's text from before the edit.
#[test]
fn escape_closes_the_autocomplete_first() {
    let mut rig = Rig::auto(&TAGS);
    rig.text = "Beach".into();
    rig.select_all_by_keys();
    rig.type_text("su");
    rig.key(Key::ArrowDown, Modifiers::NONE);
    rig.key(Key::Escape, Modifiers::NONE);
    assert!(rig.rows().is_empty() && !rig.open, "the list closed");
    assert_eq!((rig.text.as_str(), rig.endings.clone()), ("su", vec![]), "the edit goes on");
    assert!(rig.focused() && rig.editing);
    rig.type_text("n");
    assert_eq!(rig.rows(), vec![(2, false)], "typing opens it again, nothing highlighted");
    rig.key(Key::Escape, Modifiers::NONE);
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!((rig.text.as_str(), rig.endings.clone()), ("Beach", vec![Ending::Escape]));
    assert!(rig.picks.is_empty());
}

/// A long list shows eight rows and scrolls: the highlighted row is brought into view.
#[test]
fn a_long_list_scrolls_to_the_highlight() {
    let options: Vec<String> = (0..30).map(|i| format!("Tag {i:02}")).collect();
    let refs: Vec<&str> = options.iter().map(String::as_str).collect();
    let mut rig = Rig::auto(&refs);
    rig.click(FIELD, PointerButton::Primary);
    rig.type_text("tag");
    assert_eq!(rig.rows().len(), 8, "eight rows visible: {:?}", rig.rows());
    for _ in 0..12 {
        rig.key(Key::ArrowDown, Modifiers::NONE);
    }
    assert!(rig.rows().contains(&(11, true)), "the twelfth row is in view and highlighted: {:?}", rig.rows());
    assert!(!rig.rows().iter().any(|r| r.0 == 0), "the first scrolled away");
    assert_eq!(registry::unregistered(&rig.ctx), [], "the list's bar takes no drags");
}

/// The list stays inside the window: below the field when there is room, above it at the bottom
/// edge, at both window sizes; the field's menu does too.
#[test]
fn the_list_and_the_menu_stay_inside_the_window() {
    for (w, h) in [(1400u32, 900u32), (900, 600)] {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32));
        for (at, below) in [(pos2(20.0, 20.0), true), (pos2(w as f32 - 150.0, h as f32 - 34.0), false)] {
            let mut rig = Rig::with(Kind::Auto(TAGS.iter().map(|s| s.to_string()).collect()), "", (w, h), at);
            rig.click(FIELD, PointerButton::Primary);
            rig.type_text("s");
            let field = rig.rect(FIELD).unwrap();
            let rows: Vec<Rect> = rig.rows().iter().map(|(i, _)| rig.rect(&format!("{FIELD}/option/{i}")).unwrap()).collect();
            assert_eq!(rows.len(), 4, "{w}×{h}: Summer, Summer trip, Sunset, Lisbon summer");
            for r in &rows {
                assert!(screen.contains_rect(*r), "{w}×{h}: row {r:?} leaves the window");
                assert_eq!(r.min.y >= field.max.y, below, "{w}×{h}: rows below the field: {below}, row {r:?} field {field:?}");
            }
            rig.key(Key::Escape, Modifiers::NONE);
            rig.click(FIELD, PointerButton::Secondary);
            for item in ["cut", "copy", "paste", "selectAll"] {
                let r = rig.rect(&format!("{FIELD}/{item}")).unwrap();
                assert!(screen.contains_rect(r), "{w}×{h}: menu item {item} {r:?} leaves the window");
            }
        }
    }
}

// ---- The gallery ----

/// One state of the gallery: a field in a tile, driven into the state.
fn tile(name: &'static str) -> (Rig, Image) {
    const TILE: (u32, u32) = (260, 190);
    let at = pos2(10.0, 22.0);
    let mut rig = match name {
        "text" | "selected" | "menu" => Rig::with(Kind::Text, "Lisbon weekend", TILE, at),
        "search" => Rig::with(Kind::Search, "beach", TILE, at),
        _ => Rig::with(Kind::Auto(TAGS.iter().map(|s| s.to_string()).collect()), "", TILE, at),
    };
    rig.width = 220.0;
    rig.caption = Some(name);
    match name {
        "selected" => {
            rig.select_on_focus = true;
            focus_field(&rig.ctx, FIELD);
            rig.settle();
        }
        "menu" => {
            rig.select_all_by_keys();
            let f = rig.rect(FIELD).unwrap();
            rig.click_at(pos2(f.min.x + 40.0, f.center().y), PointerButton::Secondary);
        }
        "search" => rig.click(FIELD, PointerButton::Primary),
        "list" => {
            rig.click(FIELD, PointerButton::Primary);
            rig.type_text("su");
            let hover = rig.rect(&format!("{FIELD}/option/2")).unwrap().center();
            rig.frame(vec![Event::PointerMoved(hover)]);
            rig.settle();
        }
        "highlight" => {
            rig.click(FIELD, PointerButton::Primary);
            rig.type_text("su");
            rig.key(Key::ArrowDown, Modifiers::NONE);
            rig.key(Key::ArrowDown, Modifiers::NONE);
        }
        _ => {}
    }
    let image = rig.render();
    (rig, image)
}

const STATES: [&str; 6] = ["text", "selected", "menu", "search", "list", "highlight"];

/// The sheet of states: 3 × 2 tiles, 1 px `divider` lines between them.
fn sheet() -> (Image, Vec<(&'static str, Rig)>) {
    let tiles: Vec<(&str, Rig, Image)> = STATES
        .iter()
        .map(|&n| {
            let (rig, img) = tile(n);
            (n, rig, img)
        })
        .collect();
    let (tw, th) = (tiles[0].2.width, tiles[0].2.height);
    let (w, h) = (3 * tw + 2, 2 * th + 1);
    let mut rgba: Vec<u8> = Tokens::default().divider.to_array().repeat((w * h) as usize);
    for (i, (_, _, img)) in tiles.iter().enumerate() {
        let (cx, cy) = ((i as u32 % 3) * (tw + 1), (i as u32 / 3) * (th + 1));
        for y in 0..th {
            let src = (y * tw * 4) as usize..((y + 1) * tw * 4) as usize;
            let dst = (((cy + y) * w + cx) * 4) as usize;
            rgba[dst..dst + src.len()].copy_from_slice(&img.rgba[src]);
        }
    }
    (Image { width: w, height: h, rgba }, tiles.into_iter().map(|(n, r, _)| (n, r)).collect())
}

/// The window at `size` with a field at its bottom-right corner, its list open above it.
fn window(size: (u32, u32)) -> Image {
    let at = pos2(size.0 as f32 - 236.0, size.1 as f32 - 40.0);
    let mut rig = Rig::with(Kind::Auto(TAGS.iter().map(|s| s.to_string()).collect()), "", size, at);
    rig.width = 220.0;
    rig.click(FIELD, PointerButton::Primary);
    rig.type_text("s");
    rig.key(Key::ArrowDown, Modifiers::NONE);
    rig.render()
}

/// Every state renders identically twice, registers its ids and matches its fingerprint; so does
/// the window.
#[test]
fn every_state_renders_identically_twice_and_matches_its_fingerprint() {
    let (a, rigs) = sheet();
    let (b, _) = sheet();
    assert!(a.rgba == b.rgba, "two renders of the sheet differ");
    for (name, rig) in &rigs {
        assert!(rig.widget(FIELD).is_some(), "{name}: the field registers");
        assert_eq!(registry::unregistered(&rig.ctx), [], "{name}: interactive widgets without an id");
        for w in &rig.widgets {
            assert!(registry::is_valid_id(&w.id), "{name}: {:?} breaks the id grammar", w.id);
        }
        match *name {
            "menu" => assert_eq!(rig.widget(&format!("{FIELD}/copy")).map(|w| w.enabled), Some(true), "copy acts on the selection"),
            "list" => assert_eq!(rig.rows().len(), 4),
            "highlight" => assert!(rig.rows().contains(&(1, true))),
            _ => {}
        }
    }
    let mut failures = Vec::new();
    check("fields-states", &a, &mut failures);
    // The window at one size: one fingerprint per state (the app's tests check the layout of
    // the small window).
    let (one, two) = (window((1400, 900)), window((1400, 900)));
    assert!(one.rgba == two.rgba, "two renders of the window differ");
    check("fields-window-1400x900", &one, &mut failures);
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}
