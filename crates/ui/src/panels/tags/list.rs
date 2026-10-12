//! The Tags pane's "Tags in the selection:" (`organizing.md` §5.2): every tag of the selected
//! photos in alphabetical order, a tag that only some of them hold with its partial count
//! ("beach (3 of 8)"), from the `tags.list {ids}` query (asked again only when the library or the
//! selection changed). Each row is `tag:<tag text>`; hovering it shows × (`tag:<tag text>/remove`),
//! which removes the tag from every selected photo (`photo.tags.remove`, one undo step); a
//! right-click opens the tag menu (`menus-and-shortcuts.md` §4): Add to All Selected
//! (`photo.tags.add`), Search for This Tag (greyed until search arrives), Remove from Selected
//! (`photo.tags.remove`) and Rename Tag… (`tag.rename`, whose dialog asks before it renames the tag
//! on every photo). With nothing selected, or no tags on the selection, the list says "None".
//!
//! The rows scroll (with the wheel) when they would push the quick tags below out of the pane.

use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::Icon;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, CornerRadius, Id, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::menu_catalog::Mark;
use crate::menus::{self, ItemNode, Node};
use crate::state::DialogKind;

/// The room the rows leave below them for the quick tags (points).
const QUICK_ROOM: f32 = 150.0;
/// The edge of a row's × (points).
const REMOVE: f32 = 18.0;
/// The command Search for This Tag will run (M9); greyed until then.
pub const SEARCH_TAG: &str = "search.tag";
/// Why Search for This Tag is greyed.
pub const SEARCH_LATER: &str = "Not available yet.";
/// Why Add to All Selected is greyed when every selected photo holds the tag.
pub const ALL_HOLD: &str = "Every selected photo has this tag.";
/// Why the tag menu's items are greyed with nothing selected.
pub const SELECT_FIRST: &str = "Select some photos first.";

/// One row of the list: a tag held by `count` of the `of` selected photos, and its label
/// ("beach", or "beach (3 of 8)" when only some hold it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub tag: String,
    pub count: usize,
    pub of: usize,
    pub label: String,
}

impl Row {
    /// Only some of the selected photos hold the tag.
    pub fn partial(&self) -> bool {
        self.count < self.of
    }
}

/// A row's automation id: `tag:<tag text>`.
pub fn row_id(tag: &str) -> String {
    format!("tag:{tag}")
}

/// A row's ×: `tag:<tag text>/remove`.
pub fn remove_id(tag: &str) -> String {
    format!("tag:{tag}/remove")
}

/// The rows `tags.list {ids}` answered, and what they were asked for.
#[derive(Clone, Default)]
struct Cached {
    revision: Option<u64>,
    ids: Vec<PhotoId>,
    rows: Arc<Vec<Row>>,
}

/// The rows of a `tags.list {ids}` reply; entries that are not as documented are left out.
pub fn rows_of(reply: &Value) -> Vec<Row> {
    let Some(tags) = reply.get("tags").and_then(Value::as_array) else { return Vec::new() };
    tags.iter()
        .filter_map(|t| {
            let count = usize::try_from(t.get("count")?.as_u64()?).ok()?;
            let of = usize::try_from(t.get("of")?.as_u64()?).ok()?;
            Some(Row { tag: t.get("tag")?.as_str()?.to_owned(), count, of, label: t.get("label")?.as_str()?.to_owned() })
        })
        .collect()
}

/// The tags of the selected photos, asked of `tags.list` only when the library or the selection
/// changed (compared in place, so an unchanged frame copies no list). Photos the library no longer
/// has are left out; empty before the library is open.
pub fn rows(app: &mut App, ctx: &egui::Context) -> Arc<Vec<Row>> {
    let key = Id::new("tags.list.rows");
    let Some(revision) = app.session().map(|s| s.library().revision()) else { return Arc::default() };
    let selection = &app.state().selection.ids;
    let hit = ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_default::<Cached>(key);
        (c.revision == Some(revision) && c.ids == *selection).then(|| Arc::clone(&c.rows))
    });
    if let Some(rows) = hit {
        return rows;
    }
    let ids = selection.clone();
    let Some(session) = app.session_mut() else { return Arc::default() };
    let known: Vec<PhotoId> = ids.iter().copied().filter(|id| session.library().photo(*id).is_some()).collect();
    let rows =
        if known.is_empty() { Vec::new() } else { session.execute("tags.list", &json!({ "ids": known })).map(|v| rows_of(&v)).unwrap_or_default() };
    let rows = Arc::new(rows);
    ctx.data_mut(|d| d.insert_temp(key, Cached { revision: Some(revision), ids, rows: Arc::clone(&rows) }));
    rows
}

/// The tag menu of `row` over the `selected` photos (`menus-and-shortcuts.md` §4), each item
/// carrying the parameters it runs with (a whole tag, so a tag holding a comma stays one).
pub fn menu(app: &App, row: &Row, selected: &[PhotoId]) -> Vec<Node> {
    let ids = json!({ "ids": selected, "tags": [row.tag] });
    let none = selected.is_empty();
    let add = if none {
        Err(SELECT_FIRST.to_owned())
    } else if !row.partial() {
        Err(ALL_HOLD.to_owned())
    } else {
        Ok(())
    };
    let remove = if none { Err(SELECT_FIRST.to_owned()) } else { Ok(()) };
    let items: [(&'static str, &'static str, Value, Result<(), String>); 4] = [
        ("photo.tags.add", "Add to All Selected", ids.clone(), add),
        (SEARCH_TAG, "Search for This Tag", json!({ "tag": row.tag }), Err(SEARCH_LATER.to_owned())),
        ("photo.tags.remove", "Remove from Selected", ids, remove),
        ("tag.rename", "Rename Tag…", json!({ "tag": row.tag }), crate::dialogs::can_open(app, DialogKind::RenameTag)),
    ];
    items
        .into_iter()
        .map(|(id, label, params, ok)| {
            let reason = ok.err();
            Node::Item(ItemNode {
                id,
                label: label.to_owned(),
                catalog_label: label,
                params,
                widget: format!("menuitem:{id}"),
                shortcut: None,
                live: id != SEARCH_TAG,
                enabled: reason.is_none(),
                reason,
                mark: Mark::None,
                checked: false,
                dialog: (id == "tag.rename").then_some("renameTag"),
            })
        })
        .collect()
}

/// Paints `row`'s label left-aligned in `rect`: the tag in `text`, a partial count after it in
/// `text_dim`. A tag too long for the room is cut with "…", the count kept whole.
fn paint_label(app: &App, painter: &egui::Painter, row: &Row, rect: Rect) {
    let t = app.tokens();
    let font = fonts::regular(t.body);
    let one_line = |text: &str, color, max: f32| {
        let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font.clone(), color));
        job.wrap = TextWrapping::truncate_at_width(if max.is_finite() { max.max(1.0) } else { 1.0 });
        painter.layout_job(job)
    };
    let (tag, count) = match row.label.strip_prefix(row.tag.as_str()).filter(|c| !c.is_empty()) {
        Some(count) => (row.tag.as_str(), Some(painter.layout_no_wrap(count.to_owned(), font.clone(), t.text_dim))),
        None => (row.label.as_str(), None),
    };
    let count_w = count.as_ref().map_or(0.0, |c| c.size().x);
    let galley = one_line(tag, t.text, rect.width() - count_w);
    let at = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), rect).min.round();
    let end = at.x + galley.size().x;
    painter.galley(at, galley, t.text);
    if let Some(count) = count {
        let y = Align2::LEFT_CENTER.align_size_within_rect(count.size(), rect).min.y;
        painter.galley(pos2(end, y).round(), count, t.text_dim);
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    super::caption(app, ui, "Tags in the selection:");
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let rows = rows(app, &ctx);
    if rows.is_empty() {
        ui.label(RichText::new("None").font(fonts::regular(t.body)).color(t.text_dim));
        return;
    }
    let row_h = t.metrics.interact_h;
    let width = ui.available_width();
    let height = (ui.available_height() - QUICK_ROOM).max(3.0 * row_h);
    let mut remove: Option<String> = None;
    let mut opened: Option<(egui::Response, Row)> = None;
    // The wheel scrolls the rows; the bar only shows where they are (a bar that takes drags would
    // be a control without an automation id).
    let scroll = egui::ScrollArea::vertical()
        .id_salt("tags.list")
        .max_height(height)
        .auto_shrink([false, true])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL);
    scroll.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let clip = ui.clip_rect();
        for row in rows.iter() {
            let (rect, _) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
            if !rect.intersects(clip) {
                continue;
            }
            let r = ui.interact(rect, Id::new(("tags.row", row.tag.as_str())), Sense::click());
            let menu_open = r.context_menu_opened();
            let over = ui.rect_contains_pointer(rect.intersect(clip));
            let painter = ui.painter();
            if over || menu_open {
                painter.rect_filled(rect, CornerRadius::same(t.radius), t.hover);
            }
            let right = if over { REMOVE + 6.0 } else { 4.0 };
            let text = Rect::from_min_max(pos2(rect.min.x + 4.0, rect.min.y), pos2(rect.max.x - right, rect.max.y));
            paint_label(app, painter, row, text);
            widgets::register(&ctx, &row_id(&row.tag), Role::Other, &row.label, Target::Widget(&r), menu_open);
            if over {
                let x = Rect::from_center_size(pos2(rect.max.x - 2.0 - REMOVE / 2.0, rect.center().y), Vec2::splat(REMOVE));
                let mut child = ui.new_child(UiBuilder::new().max_rect(x));
                let tip = "Remove the tag from the selected photos";
                if widgets::icon_button(&mut child, &t, &remove_id(&row.tag), Icon::Close, REMOVE, false, true, tip).clicked() {
                    remove = Some(row.tag.clone());
                }
            }
            if r.secondary_clicked() || menu_open {
                opened = Some((r, row.clone()));
            }
        }
    });
    let selected = app.state().selection.ids.clone();
    if let Some(tag) = remove {
        // A refusal is on the status line.
        let _ = app.run("photo.tags.remove", &json!({ "ids": selected, "tags": [tag] }));
    }
    if let Some((response, row)) = opened {
        let nodes = menu(app, &row, &selected);
        menus::context_menu(&response, app, &nodes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_come_from_the_reply_and_odd_entries_are_left_out() {
        let reply = json!({ "photos": 8, "tags": [
            { "tag": "beach", "count": 3, "of": 8, "partial": true, "label": "beach (3 of 8)" },
            { "tag": "sea", "count": 8, "of": 8, "partial": false, "label": "sea" },
            { "tag": 3, "count": 1, "of": 8, "label": "x" },
            { "tag": "y", "count": -1, "of": 8, "label": "y" },
            "z",
        ] });
        let rows = rows_of(&reply);
        assert_eq!(rows.iter().map(|r| (r.tag.as_str(), r.partial())).collect::<Vec<_>>(), [("beach", true), ("sea", false)]);
        assert_eq!(rows_of(&json!({})), []);
        assert_eq!(rows_of(&json!({ "tags": "beach" })), []);
        assert_eq!((row_id("Summer trip"), remove_id("a/b")), ("tag:Summer trip".to_string(), "tag:a/b/remove".to_string()));
    }

    #[test]
    fn the_tag_menu_greys_what_cannot_run() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(2);
        let app = App::new(Some(s));
        let row = |count| Row { tag: "beach, sea".into(), count, of: 2, label: String::new() };
        let enabled = |nodes: Vec<Node>| -> Vec<(String, bool)> {
            nodes
                .into_iter()
                .filter_map(|n| match n {
                    Node::Item(i) => Some((i.widget, i.enabled)),
                    _ => None,
                })
                .collect()
        };
        let widget = |s: &str, on: bool| (s.to_string(), on);
        assert_eq!(
            enabled(menu(&app, &row(2), &ids)),
            [
                widget("menuitem:photo.tags.add", false),
                widget("menuitem:search.tag", false),
                widget("menuitem:photo.tags.remove", true),
                widget("menuitem:tag.rename", true),
            ]
        );
        assert!(enabled(menu(&app, &row(1), &ids))[0].1, "partial: Add to All Selected");
        let none = enabled(menu(&app, &row(1), &[]));
        assert!(!none[0].1 && !none[2].1 && none[3].1, "nothing selected: only Rename Tag…");
        let Some(Node::Item(add)) = menu(&app, &row(1), &ids).into_iter().next() else { panic!("an item") };
        assert_eq!(add.params["tags"], json!(["beach, sea"]), "a whole tag, never split at its comma");
    }
}
