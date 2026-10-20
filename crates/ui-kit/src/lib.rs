//! dalica-ui-kit: The generic look: `Tokens`, widgets, icons drawn in code, the widget registry and the CPU rasterizer for screenshots.
//!
//! Layer L5; see `docs/architecture.md` §2 and `docs/design-system.md`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod fonts;
pub mod icons;
pub mod input;
pub mod linebreak;
pub mod raster;
pub mod registry;
pub mod text;
pub mod theme;
pub mod widgets;

pub use icons::Icon;
pub use raster::{Image, RasterError, Rasterizer};
pub use registry::{Role, WidgetInfo};
pub use theme::Tokens;

/// Renders `ui` headlessly into a `width × height` pixel image at `pixels_per_point`, with the
/// theme of `tokens` applied: a fresh context, two passes (layout settles on the second), painted
/// by the CPU [`Rasterizer`]. The clock is fixed, so the same `ui` gives the same pixels.
pub fn snapshot(width: u32, height: u32, pixels_per_point: f32, tokens: &Tokens, mut ui: impl FnMut(&mut egui::Ui)) -> Result<Image, RasterError> {
    let ctx = egui::Context::default();
    tokens.apply(&ctx);
    let mut raster = Rasterizer::new();
    let mut last = None;
    for _ in 0..2 {
        let out = ctx.run_ui(raw_input(width, height, pixels_per_point), &mut ui);
        last = Some(paint(&ctx, &mut raster, out, width, height, tokens.chrome)?);
    }
    last.ok_or(RasterError::Size(width, height))
}

/// The input of one headless pass: a `width × height` pixel screen at `pixels_per_point`, at time 0.
pub fn raw_input(width: u32, height: u32, pixels_per_point: f32) -> egui::RawInput {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 { pixels_per_point } else { 1.0 };
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32 / ppp, height as f32 / ppp))),
        time: Some(0.0),
        max_texture_side: Some(raster::MAX_SIDE as usize),
        focused: true,
        ..Default::default()
    };
    input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
    input
}

/// Paints one pass's output: uploads its textures, tessellates, rasterizes, frees.
pub fn paint(
    ctx: &egui::Context,
    raster: &mut Rasterizer,
    out: egui::FullOutput,
    width: u32,
    height: u32,
    clear: egui::Color32,
) -> Result<Image, RasterError> {
    let mut delta = out.textures_delta;
    raster.set_textures(&mut delta);
    let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
    let image = raster.render(&primitives, out.pixels_per_point, width, height, clear);
    raster.free_textures(&mut delta);
    image
}
