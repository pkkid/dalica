//! The map tiles over the offline layer (`organizing.md` §7, reserved decision 4): the pane's
//! one-time question, the tiles the session fetched for the visible box with the provider's
//! attribution, and the session told whether the pane is open.
//!
//! - **Open or closed.** Each pass that draws the pane tells the session it is open
//!   (`Session::set_places_open(true)`); [`end_frame`], after the panels, tells it the pane closed
//!   when a pass did not draw it (closed, another pane, a room in front), which cancels the tiles on
//!   their way and forgets the textures. Nothing is fetched while it is closed.
//! - **The question.** While Allow map tiles is off and the question was never answered
//!   ([`crate::commands::map::asks`]), a card at the bottom of the map asks "Show map tiles from
//!   %provider%? Dalica fetches map pictures from the internet while this pane is open." with
//!   [Allow] (`button:map.allowTiles`, the default) and [Not Now] (`button:map.notNow`); both run
//!   `map.allowTiles`. Registered as `map:places/question`, labelled with its question. The
//!   provider is OpenStreetMap for the default server, else the tile server's host.
//! - **The tiles.** With tiles allowed, the tiles covering the map's visible box at its zoom are
//!   asked for (`Session::request_map_tiles`) when that set of tiles changes, never every pass; the
//!   tiles finished are taken each pass (`Session::take_map_tiles`) and kept as textures (at most
//!   [`MAX_TEXTURES`], the least recently drawn dropped first), drawn over the offline layer at the
//!   map's zoom and coarser ones (a coarser tile fills in while finer ones arrive). A placeholder
//!   (a tile too large, undecodable or not fetched) draws nothing: the offline map shows through.
//!   While any tile shows, the attribution line is the provider's (`network.tileAttribution`); with
//!   none, the offline layer's; it registers as `text:places.attribution`, labelled with its text.
//!   Changing the tile server forgets the textures.
//!
//! The textures live in the egui context's memory (the UI's own, never the view state); nothing
//! here touches the disk or the network: the session's workers fetch and decode.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dalica_core::geo::{Tile, tiles_covering};
use dalica_engine::map_tiles::DEFAULT_TILE_SERVER;
use dalica_image::Rgba8;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, MAP_PAD, MapOut, MapTile, Target, primary_button, text_button};
use dalica_ui_kit::{Tokens, fonts};
use egui::{
    Align, ColorImage, Context, CornerRadius, Id, Layout, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder, pos2, vec2,
};
use serde_json::json;

use crate::app::App;
use crate::commands::map::asks;

/// The question's card and its buttons.
pub const QUESTION_ID: &str = "map:places/question";
pub const ALLOW_ID: &str = "button:map.allowTiles";
pub const NOT_NOW_ID: &str = "button:map.notNow";
pub const ALLOW: &str = "Allow";
pub const NOT_NOW: &str = "Not Now";
/// The map's attribution line.
pub const ATTRIBUTION_ID: &str = "text:places.attribution";
/// The question's second sentence.
pub const DETAIL: &str = "Dalica fetches map pictures from the internet while this pane is open.";
/// The default server's provider, as the question names it.
pub const DEFAULT_PROVIDER: &str = "OpenStreetMap";
/// The most tile textures kept.
pub const MAX_TEXTURES: usize = 384;
/// How often a pass looks for tiles while some are on their way.
const POLL: Duration = Duration::from_millis(50);
/// The card's padding, the gap between its text and its buttons, and the room kept for the
/// attribution line under it (points).
const CARD_PAD: f32 = 10.0;
const CARD_GAP: f32 = 8.0;
const ATTRIBUTION_ROOM: f32 = 20.0;

/// What the map draws this pass: the tile textures and the attribution (`None`: the offline
/// layer's).
#[derive(Default)]
pub struct Layer {
    pub tiles: Vec<MapTile>,
    pub attribution: Option<String>,
}

/// The textures and what was last asked for, kept between passes.
#[derive(Default)]
struct Cache {
    /// The tile server the textures came from.
    server: String,
    /// Each tile's texture and the pass it was last drawn.
    textures: HashMap<Tile, (TextureHandle, u64)>,
    /// The tiles last asked for: the server, the zoom and the tiles.
    asked: Option<(String, u8, Vec<Tile>)>,
    /// The last pass that drew the pane.
    drawn: Option<u64>,
}

impl Cache {
    fn forget(&mut self) {
        self.textures.clear();
        self.asked = None;
    }
}

type Shared = Arc<Mutex<Cache>>;

fn cache(ctx: &Context) -> Shared {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Shared>(Id::new("places.tiles")).clone())
}

fn lock(c: &Shared) -> std::sync::MutexGuard<'_, Cache> {
    c.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The provider the question names for tile server `template`: OpenStreetMap for the default,
/// else the server's host ("the tile server" when there is none).
pub fn provider(template: &str) -> String {
    if template == DEFAULT_TILE_SERVER {
        return DEFAULT_PROVIDER.into();
    }
    let rest = template.split_once("://").map_or(template, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or_default();
    if host.is_empty() { "the tile server".into() } else { host.to_string() }
}

/// The question for tile server `template`.
pub fn question(template: &str) -> String {
    format!("Show map tiles from {}? {DETAIL}", provider(template))
}

/// `image` as a texture, `None` when its size and its pixels disagree.
fn texture(ctx: &Context, tile: Tile, image: &Rgba8) -> Option<TextureHandle> {
    let (w, h) = (usize::try_from(image.width).ok()?, usize::try_from(image.height).ok()?);
    if w == 0 || h == 0 || w.checked_mul(h)?.checked_mul(4)? != image.data.len() {
        return None;
    }
    let pixels = ColorImage::from_rgba_unmultiplied([w, h], &image.data);
    Some(ctx.load_texture(format!("places.tile:{}/{}/{}", tile.zoom, tile.x, tile.y), pixels, TextureOptions::LINEAR))
}

/// The pane is drawn this pass: tells the session it is open, takes the tiles finished since the
/// last pass and gives the map the textures to draw at `zoom` and coarser, with the attribution.
pub fn layer(app: &mut App, ctx: &Context, zoom: u8) -> Layer {
    let shared = cache(ctx);
    let mut c = lock(&shared);
    let pass = ctx.cumulative_pass_nr();
    c.drawn = Some(pass);
    let Some(s) = app.session_mut() else { return Layer::default() };
    s.set_places_open(true);
    if !s.map_tiles_allowed() {
        c.forget();
        return Layer::default();
    }
    let n = s.settings().network();
    if c.server != n.tile_server {
        c.forget();
        c.server.clone_from(&n.tile_server);
    }
    for t in s.take_map_tiles() {
        // A placeholder draws nothing: the offline layer shows through.
        if let Ok(image) = &t.image
            && let Some(handle) = texture(ctx, t.tile, image)
        {
            c.textures.insert(t.tile, (handle, pass));
        }
    }
    if s.map_tiles().pending() > 0 {
        ctx.request_repaint_after(POLL);
    }
    let mut tiles = Vec::new();
    for (tile, (handle, used)) in &mut c.textures {
        if tile.zoom <= zoom {
            *used = pass;
            tiles.push(MapTile { tile: *tile, texture: handle.id() });
        }
    }
    if c.textures.len() > MAX_TEXTURES {
        let mut by_use: Vec<(u64, Tile)> = c.textures.iter().map(|(t, (_, u))| (*u, *t)).collect();
        by_use.sort_unstable();
        let extra = c.textures.len() - MAX_TEXTURES;
        for (_, t) in by_use.into_iter().take(extra) {
            c.textures.remove(&t);
        }
        tiles.retain(|m| c.textures.contains_key(&m.tile));
    }
    let attribution = (!tiles.is_empty()).then(|| n.tile_attribution.clone());
    Layer { tiles, attribution }
}

/// After the map: registers its attribution line (`attribution`, `None`: the offline layer's),
/// asks for the tiles of its visible box when they changed, and draws the question over the map's
/// `rect` while it applies.
pub fn after_map(app: &mut App, ui: &mut Ui, rect: Rect, out: &MapOut, attribution: Option<&str>) {
    let t = *app.tokens();
    register_attribution(ui, &t, rect, attribution.filter(|a| !a.is_empty()).unwrap_or(widgets::OFFLINE_ATTRIBUTION));
    let shared = cache(ui.ctx());
    if let Some(s) = app.session_mut()
        && s.map_tiles_allowed()
        && let Some(area) = out.visible
    {
        let zoom = out.view.zoom();
        let server = s.settings().network().tile_server;
        let mut c = lock(&shared);
        if let Ok(tiles) = tiles_covering(area, zoom) {
            let key = (server, zoom, tiles);
            if c.asked.as_ref() != Some(&key) {
                if let Err(e) = s.request_map_tiles(area, zoom) {
                    log::debug!("map tiles not asked for: {e}");
                }
                c.asked = Some(key);
                ui.ctx().request_repaint_after(POLL);
            }
        }
    }
    if asks(app) {
        let server = app.session().map(|s| s.settings().network().tile_server).unwrap_or_default();
        let mut answer = None;
        card(ui, &t, rect, &question(&server), &mut answer);
        if let Some(on) = answer {
            // A refusal's reason is on the status line.
            let _ = app.run("map.allowTiles", &json!({ "on": on }));
        }
    }
}

/// Registers the map's attribution line `text:places.attribution` (labelled with its text) where
/// the map widget paints it: bottom right, on its plate.
fn register_attribution(ui: &Ui, t: &Tokens, rect: Rect, text: &str) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), fonts::regular(t.small), t.text_label);
    let size = vec2(galley.size().x.min((rect.width() - 2.0 * MAP_PAD).max(1.0)), galley.size().y) + vec2(8.0, 4.0);
    let plate = Rect::from_min_size(rect.max - size, size).intersect(rect);
    widgets::register(ui.ctx(), ATTRIBUTION_ID, Role::Other, text, Target::Painted { rect: plate, clip: ui.clip_rect() }, false);
}

/// The question's card at the bottom of the map `area` (above the attribution line; at the top
/// when the map is too short), in the bubble's popup frame; `answer` is set on a click.
fn card(ui: &mut Ui, t: &Tokens, area: Rect, text: &str, answer: &mut Option<bool>) {
    let w = (area.width() - 2.0 * MAP_PAD).min(widgets::BUBBLE_MAX_W + 40.0);
    if w < 60.0 {
        return;
    }
    let galley = ui.painter().layout(text.to_owned(), fonts::regular(t.body), t.text, w - 2.0 * CARD_PAD);
    let h = (2.0 * CARD_PAD + galley.size().y + CARD_GAP + t.metrics.button_h).ceil();
    let bottom = area.max.y - MAP_PAD - ATTRIBUTION_ROOM;
    let top = if bottom - h >= area.min.y + MAP_PAD { bottom - h } else { area.min.y + MAP_PAD };
    let rect = Rect::from_min_size(pos2(area.center().x - w / 2.0, top), vec2(w, h));
    // The card takes the clicks on it, so none reaches the map under it.
    let r = ui.interact(rect, Id::new(QUESTION_ID), Sense::click());
    let p = ui.painter_at(area);
    let radius = CornerRadius::same(6);
    p.add(t.popup_shadow.as_shape(rect, radius));
    p.rect(rect, radius, t.chrome, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let text_h = galley.size().y;
    p.galley(pos2(rect.min.x + CARD_PAD, rect.min.y + CARD_PAD), galley, t.text);
    let row = Rect::from_min_max(
        pos2(rect.min.x + CARD_PAD, rect.min.y + CARD_PAD + text_h + CARD_GAP),
        pos2(rect.max.x - CARD_PAD, rect.max.y - CARD_PAD),
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::right_to_left(Align::Center)));
    child.set_clip_rect(area.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
    // Right to left: [Allow] [Not Now] reads Allow first, Not Now at the right edge.
    if text_button(&mut child, t, NOT_NOW_ID, NOT_NOW, false).clicked() {
        *answer = Some(false);
    }
    if primary_button(&mut child, t, ALLOW_ID, ALLOW).clicked() {
        *answer = Some(true);
    }
    widgets::register(ui.ctx(), QUESTION_ID, Role::Other, text, Target::Widget(&r), false);
}

/// The pane is drawn this pass (with or without room for its map): the session is told it is open.
pub fn show(app: &mut App, ui: &mut Ui) {
    let shared = cache(ui.ctx());
    lock(&shared).drawn = Some(ui.ctx().cumulative_pass_nr());
    if let Some(s) = app.session_mut() {
        s.set_places_open(true);
    }
}

/// After the panels of a pass: a pass that did not draw the Places pane tells the session it is
/// closed (the tiles on their way are cancelled) and forgets the textures.
pub fn end_frame(app: &mut App, ctx: &Context) {
    let shared = cache(ctx);
    let mut c = lock(&shared);
    if c.drawn == Some(ctx.cumulative_pass_nr()) {
        return;
    }
    c.forget();
    c.drawn = None;
    if let Some(s) = app.session_mut()
        && s.map_tiles().places_open()
    {
        s.set_places_open(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_names_the_provider() {
        assert_eq!(provider(DEFAULT_TILE_SERVER), "OpenStreetMap");
        assert_eq!(provider("https://tiles.example.org/{z}/{x}/{y}.png"), "tiles.example.org");
        assert_eq!(provider("https://me:pw@maps.example:8443/t?z={z}&x={x}&y={y}"), "maps.example");
        assert_eq!(provider(""), "the tile server");
        assert_eq!(question(DEFAULT_TILE_SERVER), format!("Show map tiles from OpenStreetMap? {DETAIL}"));
    }

    #[test]
    fn a_picture_whose_size_and_pixels_disagree_is_no_texture() {
        let ctx = Context::default();
        let tile = Tile::new(1, 0, 0).unwrap();
        assert!(texture(&ctx, tile, &Rgba8 { width: 2, height: 2, data: vec![0; 16] }).is_some());
        assert!(texture(&ctx, tile, &Rgba8 { width: 2, height: 2, data: vec![0; 15] }).is_none());
        assert!(texture(&ctx, tile, &Rgba8 { width: 0, height: 2, data: Vec::new() }).is_none());
        assert!(texture(&ctx, tile, &Rgba8 { width: u32::MAX, height: u32::MAX, data: Vec::new() }).is_none());
    }
}
