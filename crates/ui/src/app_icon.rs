//! The app icon inside the window (`docs/assets.md` §7): at the title bar's top-left corner
//! (`design-system.md` §11.1) and on the About tab. It is drawn from the hicolor renders committed
//! in `assets/app-icon/` (`packaging/icons.sh` makes them from `dalica.svg`), never redrawn in
//! code, so the window shows the same icon as the task bar and the installers.
//!
//! The renders are embedded in the binary. The one drawn is the smallest at least as large as the
//! icon's edge in physical pixels (16 pt at 1× is the 16 px render, pixel for pixel), so the icon
//! stays crisp at every display scale. Each render is decoded and uploaded as a texture once per
//! egui context, the first time it is shown; a render that does not decode is logged once and
//! leaves the icon's place empty, never an error.

use egui::emath::GuiRounding;
use egui::{Color32, Context, Id, Painter, Rect, TextureHandle, TextureOptions, pos2};

/// The hicolor renders, by edge in pixels, smallest first.
pub const RENDERS: [(u32, &[u8]); 7] = [
    (16, include_bytes!("../../../assets/app-icon/hicolor/16x16/apps/org.dalica.Dalica.png")),
    (24, include_bytes!("../../../assets/app-icon/hicolor/24x24/apps/org.dalica.Dalica.png")),
    (32, include_bytes!("../../../assets/app-icon/hicolor/32x32/apps/org.dalica.Dalica.png")),
    (48, include_bytes!("../../../assets/app-icon/hicolor/48x48/apps/org.dalica.Dalica.png")),
    (64, include_bytes!("../../../assets/app-icon/hicolor/64x64/apps/org.dalica.Dalica.png")),
    (128, include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/org.dalica.Dalica.png")),
    (256, include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/org.dalica.Dalica.png")),
];

/// The render to draw `px` physical pixels wide: the smallest at least that large, or the largest.
pub fn render_for(px: f32) -> (u32, &'static [u8]) {
    let [.., largest] = RENDERS;
    RENDERS.into_iter().find(|&(edge, _)| edge as f32 >= px.ceil()).unwrap_or(largest)
}

/// The texture of the app icon drawn `side` points wide on `ctx`'s display, uploaded the first time
/// it is asked for; `None` if the render does not decode.
pub fn texture(ctx: &Context, side: f32) -> Option<TextureHandle> {
    let (edge, png) = render_for(side * ctx.pixels_per_point());
    let id = Id::new(("dalica-app-icon", edge));
    if let Some(slot) = ctx.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return slot;
    }
    let handle = match decode_png(png) {
        Ok(image) => Some(ctx.load_texture(format!("dalica-app-icon-{edge}"), image, TextureOptions::LINEAR)),
        Err(e) => {
            log::warn!("dalica: the {edge} px app icon does not decode: {e}");
            None
        }
    };
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// Paints the app icon filling the square `rect` (its corners snapped to physical pixels).
pub fn paint(painter: &Painter, rect: Rect) {
    let rect = rect.round_to_pixels(painter.pixels_per_point());
    if let Some(tex) = texture(painter.ctx(), rect.width()) {
        painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

/// An 8-bit PNG as an egui image (palettes and 16-bit samples are expanded and stripped; grey and
/// RGB get an opaque alpha). At most 64 MiB of pixels.
pub fn decode_png(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("the PNG is too large")?;
    if size > 64 << 20 {
        return Err("the PNG is too large".into());
    }
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let data = buf.get(..info.buffer_size()).ok_or("the PNG is truncated")?;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("the PNG's palette was not expanded".into()),
    };
    if rgba.len() != w.saturating_mul(h).saturating_mul(4) {
        return Err("the PNG's size does not match its pixels".into());
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn committed(edge: u32) -> Vec<u8> {
        let dir = format!("{edge}x{edge}");
        let path = ["..", "..", "assets", "app-icon", "hicolor", &dir, "apps", "org.dalica.Dalica.png"]
            .iter()
            .fold(PathBuf::from(env!("CARGO_MANIFEST_DIR")), |p, part| p.join(part));
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn every_render_is_the_committed_app_icon_and_decodes_at_its_size() {
        for (edge, png) in RENDERS {
            assert_eq!(png, committed(edge).as_slice(), "the {edge} px render");
            let image = decode_png(png).unwrap();
            assert_eq!(image.size, [edge as usize, edge as usize]);
        }
        assert!(RENDERS.windows(2).all(|w| w[0].0 < w[1].0), "smallest first");
        assert!(decode_png(b"not a png").is_err());
        assert!(decode_png(&RENDERS[5].1[..RENDERS[5].1.len() / 2]).is_err());
    }

    #[test]
    fn the_render_is_the_smallest_that_covers_the_pixels() {
        assert_eq!(render_for(16.0).0, 16);
        assert_eq!(render_for(20.0).0, 24);
        assert_eq!(render_for(32.0).0, 32);
        assert_eq!(render_for(32.5).0, 48);
        assert_eq!(render_for(96.0).0, 128);
        assert_eq!(render_for(192.0).0, 256);
        assert_eq!(render_for(4000.0).0, 256);
        assert_eq!(render_for(0.0).0, 16);
        assert_eq!(render_for(f32::NAN).0, 256);
        assert_eq!(render_for(-5.0).0, 16);
    }

    #[test]
    fn a_texture_is_uploaded_once_per_size() {
        let ctx = Context::default();
        let a = texture(&ctx, 16.0).unwrap();
        let b = texture(&ctx, 16.0).unwrap();
        assert_eq!(a.id(), b.id());
        assert_eq!(a.size(), [16, 16]);
        assert_eq!(texture(&ctx, 96.0).unwrap().size(), [128, 128]);
    }
}
