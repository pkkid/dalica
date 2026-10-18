//! The effect tabs' tile grids (`editing.md` §8.1–§8.4): tabs 3–5 of the edit panel (`effects`,
//! `effects2`, `effects3`), each a fixed grid of three columns and at most four rows of tiles
//! (`effect:<name>`, ui-kit's [`effect_grid`]) in the catalogue's grid order, centred in the
//! panel, never scrolling.
//!
//! - **The previews** come from the engine's tile worker (`Session::request_effect_tiles`, M12.5):
//!   each frame the tab is shown asks for the open photo's tiles of that tab at the preview's size
//!   in device pixels (the same request again changes nothing), takes what the worker finished
//!   (`Session::take_effect_tiles`) and uploads it. A tile is `gray 38` (`placeholder`) until its
//!   preview arrives; after an edit the old preview stays until the remade one replaces it (no grey
//!   flash per edit). While tiles are on their way the frame looks again after [`POLL`]. Nothing
//!   here reads, decodes or renders.
//! - **A click** on a one-click tile (Sepia, B&W, Warmify, Invert Colors, the "1-click" badge)
//!   applies the effect as one step of the photo's history (`edit.effect.apply {name}`); on any
//!   other tile it opens the effect's panel (`edit.tool.open {tool: "effect", name}`,
//!   [`super::effect_panel`]), which takes the grid's place until Apply or Cancel. A refusal is a
//!   toast.
//! - **Disabled** while a modal tool is open (the caller disables `ui`): the previews dimmed, the
//!   labels `text_disabled`, the tooltips still shown.

use std::collections::HashMap;
use std::time::Duration;

use dalica_core::PhotoId;
use dalica_edit::catalog::{self, Effect, OpInfo};
use dalica_engine::EffectTile as Made;
use dalica_engine::effect_tiles::MAX_TILE;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{EffectTile, ToastKind, effect_grid, effect_grid_size};
use egui::{Context, Id, Rect, TextureHandle, Ui, UiBuilder, Vec2, pos2};
use serde_json::json;

use crate::app::App;
use crate::commands::edit_view::open_photo;
use crate::state::EditTab;

/// How soon the grid looks again while tiles are on their way.
pub const POLL: Duration = Duration::from_millis(33);

/// The catalogue's number of effect tab `tab` (3, 4 or 5); `None` for tabs 1 and 2.
pub fn tab_number(tab: EditTab) -> Option<u8> {
    match tab {
        EditTab::Effects => Some(3),
        EditTab::Effects2 => Some(4),
        EditTab::Effects3 => Some(5),
        EditTab::Basic | EditTab::Tuning => None,
    }
}

/// The effects of tab `n` in grid order (place 0 first), at most `catalog::MAX_TILES`.
pub fn tiles_of(n: u8) -> Vec<(&'static OpInfo, Effect)> {
    let mut rows: Vec<_> = catalog::effects(n).collect();
    rows.sort_by_key(|(_, e)| e.place);
    rows.truncate(catalog::MAX_TILES);
    rows
}

/// The height the content of edit tab `tab` needs: an effect tab's whole grid (it never scrolls);
/// 0 for tabs 1 and 2. The panel gives it room ([`super::panel::parts_for`]).
pub fn needed_height(t: &Tokens, tab: EditTab) -> f32 {
    tab_number(tab).map_or(0.0, |n| effect_grid_size(t, tiles_of(n).len()).y)
}

/// The size of a tile's preview in device pixels: `points` (`metrics.effect_preview`) at
/// `pixels_per_point`, rounded, each side 1 … `MAX_TILE`.
pub fn device_size(points: Vec2, pixels_per_point: f32) -> (u32, u32) {
    let side = |p: f32| {
        let v = (p * pixels_per_point).round();
        if v.is_finite() { (v.max(1.0) as u32).min(MAX_TILE) } else { 1 }
    };
    (side(points.x), side(points.y))
}

/// One preview uploaded.
#[derive(Clone)]
struct Texture {
    handle: TextureHandle,
    /// The device size it was made at.
    size: (u32, u32),
}

/// What the grids keep between frames (in the context's data): the photo the previews are of and
/// each tile's preview by `(tab, place)`; at most the 27 effects' previews.
#[derive(Clone, Default)]
struct Grids {
    photo: Option<PhotoId>,
    textures: HashMap<(u8, u8), Texture>,
}

/// Draws effect tab `tab`'s grid at the top of `ui`'s rect (disabled while a tool is open: the
/// caller disables `ui`).
pub fn show(app: &mut App, ui: &mut Ui, tab: EditTab) {
    let Some(n) = tab_number(tab) else { return };
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let photo = open_photo(app);
    let size = device_size(t.metrics.effect_preview, ctx.pixels_per_point());
    let id = Id::new("editEffectGrids");
    let mut grids = ctx.data_mut(|d| d.remove_temp::<Grids>(id)).unwrap_or_default();
    if grids.photo != photo {
        grids = Grids { photo, textures: HashMap::new() };
    }
    let mut pending = false;
    if let (Some(photo), Some(s)) = (photo, app.session_mut()) {
        // A photo gone from the library has no tiles: the grid stays grey.
        if s.request_effect_tiles(photo, n, size).is_ok() {
            for made in s.take_effect_tiles() {
                upload(&ctx, &mut grids, photo, &made);
            }
            pending = s.effect_tiles_pending() > 0;
        }
    }
    let rows = tiles_of(n);
    let tiles: Vec<EffectTile<'_>> = rows
        .iter()
        .map(|(op, e)| EffectTile {
            name: op.name,
            label: op.label,
            tooltip: e.tooltip,
            preview: grids.textures.get(&(n, e.place)).filter(|tex| tex.size == size).map(|tex| tex.handle.id()),
            one_click: e.one_click,
            enabled: photo.is_some(),
        })
        .collect();
    let clicked = grid(ui, &t, &tiles);
    ctx.data_mut(|d| d.insert_temp(id, grids));
    if pending {
        ctx.request_repaint_after(POLL);
    }
    if let Some((op, e)) = clicked.and_then(|i| rows.get(i)) {
        let done = if e.one_click {
            app.run("edit.effect.apply", &json!({ "name": op.name }))
        } else {
            app.run("edit.tool.open", &json!({ "tool": "effect", "name": op.name }))
        };
        if let Err(err) = done {
            app.toast(&err.to_string(), ToastKind::Caution);
        }
    }
}

/// The grid of `tiles` at the top of `ui`'s rect, centred on it: a grid wider than the rect (the
/// 266 pt grid in the panel's 264 pt content) reaches into the panel's margins rather than being
/// cut. Returns the index of the tile clicked.
fn grid(ui: &mut Ui, t: &Tokens, tiles: &[EffectTile<'_>]) -> Option<usize> {
    let area = ui.max_rect();
    let width = effect_grid_size(t, tiles.len()).x.max(area.width());
    let x0 = (area.center().x - width / 2.0).round();
    let rect = Rect::from_min_max(pos2(x0, area.min.y), pos2(x0 + width, area.max.y.max(area.min.y)));
    let clip = ui.clip_rect();
    let mut child = ui.new_child(UiBuilder::new().id_salt("editEffectGrid").max_rect(rect));
    child.set_clip_rect(Rect::from_min_max(pos2(rect.min.x, clip.min.y), pos2(rect.max.x, clip.max.y.max(clip.min.y))));
    effect_grid(&mut child, t, tiles).clicked
}

/// Uploads tile `made` into `grids` when it is of `photo`; a malformed picture is skipped.
fn upload(ctx: &Context, grids: &mut Grids, photo: PhotoId, made: &Made) {
    if made.photo != photo {
        return;
    }
    let image = &made.image;
    let dims = [image.width as usize, image.height as usize];
    if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
        return;
    }
    let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
    let size = (image.width, image.height);
    match grids.textures.get_mut(&(made.tab, made.place)) {
        Some(tex) => {
            tex.handle.set(pixels, egui::TextureOptions::LINEAR);
            tex.size = size;
        }
        None => {
            let handle = ctx.load_texture(format!("effect:{}", made.name), pixels, egui::TextureOptions::LINEAR);
            grids.textures.insert((made.tab, made.place), Texture { handle, size });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_effect_tab_lists_its_tiles_in_grid_order() {
        assert_eq!(tab_number(EditTab::Basic), None);
        assert_eq!(tab_number(EditTab::Tuning), None);
        let counts = [EditTab::Effects, EditTab::Effects2, EditTab::Effects3].map(|t| tiles_of(tab_number(t).unwrap()).len());
        assert_eq!(counts, [12, 7, 8]);
        let t = Tokens::DARK;
        assert_eq!([EditTab::Tuning, EditTab::Effects, EditTab::Effects2].map(|tab| needed_height(&t, tab)), [0.0, 288.0, 215.0]);
        for n in catalog::TABS {
            let places: Vec<u8> = tiles_of(n).iter().map(|(_, e)| e.place).collect();
            assert_eq!(places, (0..places.len() as u8).collect::<Vec<_>>(), "tab {n}");
        }
    }

    #[test]
    fn the_device_size_scales_rounds_and_stays_in_range() {
        let p = egui::vec2(78.0, 48.0);
        assert_eq!(device_size(p, 1.0), (78, 48));
        assert_eq!(device_size(p, 2.0), (156, 96));
        assert_eq!(device_size(p, 1.25), (98, 60));
        assert_eq!(device_size(p, 0.0), (1, 1));
        assert_eq!(device_size(p, f32::NAN), (1, 1));
        assert_eq!(device_size(p, 1e9), (MAX_TILE, MAX_TILE));
    }
}
