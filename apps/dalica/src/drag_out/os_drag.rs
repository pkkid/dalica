//! Photos dragged out on Windows and macOS, through the `drag` crate (CrabNebula, MIT OR
//! Apache-2.0, pinned), which holds the `unsafe` of OLE's `DoDragDrop` and of AppKit's dragging
//! source: the files as a copy, the pill as the drag's image (a PNG; its resolution says the scale,
//! so AppKit shows it at its size in points).
//!
//! On Windows the drag is modal: [`start`] returns once the drop is done, and the window, which
//! never saw the release, is given one where the drag ended (back over the window: there, an
//! inside drop; else outside it). On macOS the drag runs on after [`start`] returns and its end is
//! a callback; the window is given its release outside, so a drag brought back over Dalica drops
//! nothing (AppKit's end point is in screen points with a flipped origin, and the window's place
//! on the screen is not known in the same terms).

use std::sync::Arc;

use dalica_ui::drag::out::Outgoing;
use dalica_ui_kit::Image;

use super::{Native, Release};

/// Starts the OS's drag of `out` from the window of `frame`; `native` hears when it ends.
pub fn start(frame: &eframe::Frame, ctx: &egui::Context, out: &Outgoing, native: Arc<Native>) {
    // AppKit takes no drag without an image: a clear pixel where the pill cannot be had.
    let clear = Image { width: 1, height: 1, rgba: vec![0; 4] };
    let Some(png) = out.image.as_ref().and_then(|img| png_of(img, out.scale)).or_else(|| png_of(&clear, 1.0)) else {
        native.ended(Release { inside: None });
        return;
    };
    // The window's place, to tell a drag ended back over it (Windows' end point is in pixels).
    let (inner, ppp) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().native_pixels_per_point.unwrap_or(1.0)));
    let on_end = {
        let native = Arc::clone(&native);
        move |_result: drag::DragResult, at: drag::CursorPosition| {
            let inside = if cfg!(target_os = "windows") { inside(inner, ppp, at.x, at.y) } else { None };
            native.ended(Release { inside });
        }
    };
    let options = drag::Options { skip_animatation_on_cancel_or_failure: false, mode: drag::DragMode::Copy };
    if let Err(e) = drag::start_drag(frame, drag::DragItem::Files(out.paths.clone()), drag::Image::Raw(png), on_end, options) {
        log::warn!("dalica: the drag out did not start: {e}");
        native.ended(Release { inside: None });
    }
}

/// Where the screen pixel (`x`, `y`) is in the window whose inner rect is `inner` (points), at
/// `ppp` pixels per point; `None` when outside it or unknown.
pub fn inside(inner: Option<egui::Rect>, ppp: f32, x: i32, y: i32) -> Option<egui::Pos2> {
    let inner = inner?;
    let ppp = if ppp.is_finite() && ppp > 0.0 { ppp } else { 1.0 };
    let p = egui::pos2(x as f32 / ppp, y as f32 / ppp);
    inner.contains(p).then(|| (p - inner.min).to_pos2())
}

/// `img` (premultiplied RGBA at `scale` pixels per point) as a PNG of straight RGBA whose physical
/// pixel size says the scale (72 dpi per point).
pub fn png_of(img: &Image, scale: f32) -> Option<Vec<u8>> {
    let mut rgba = img.rgba.clone();
    for [r, g, b, a] in rgba.as_chunks_mut::<4>().0 {
        let a = u16::from(*a);
        if a > 0 && a < 255 {
            for c in [r, g, b] {
                *c = (u16::from(*c) * 255 / a).min(255) as u8;
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let per_metre = (72.0 * scale / 0.0254).round() as u32;
        enc.set_pixel_dims(Some(png::PixelDimensions { xppu: per_metre, yppu: per_metre, unit: png::Unit::Meter }));
        let mut w = enc.write_header().ok()?;
        w.write_image_data(&rgba).ok()?;
        w.finish().ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_is_a_png_of_straight_alpha_and_the_end_point_is_found_in_the_window() {
        let img = Image { width: 2, height: 1, rgba: vec![64, 32, 0, 128, 10, 20, 30, 255] };
        let png = png_of(&img, 2.0).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[..8], [127, 63, 0, 128, 10, 20, 30, 255]);
        let dims = reader.info().pixel_dims.unwrap();
        assert_eq!(dims.xppu, 5669, "144 dpi");
        let inner = Some(egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(800.0, 600.0)));
        assert_eq!(inside(inner, 2.0, 400, 200), Some(egui::pos2(100.0, 50.0)));
        assert_eq!(inside(inner, 2.0, 100, 200), None);
        assert_eq!(inside(None, 1.0, 400, 200), None);
    }
}
