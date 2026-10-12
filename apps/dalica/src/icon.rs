//! The app icon the window shows (title bar, taskbar, Alt+Tab, and the Dock when running
//! unbundled on macOS), from the renders committed in `assets/app-icon/` (`docs/assets.md` §7;
//! `packaging/icons.sh` makes them from `dalica.svg`). macOS gets the 1024 px render on Apple's
//! icon grid; elsewhere the full-tile 256 px hicolor render, which reads better at small sizes.
//! Packaged builds also carry the `.icns`, `.ico` and hicolor files (`docs/releasing.md`).
//!
//! The PNG is embedded in the binary and decoded once, when the window is made; a decode failure
//! leaves the platform's default icon (logged), never an error.

use egui::IconData;

#[cfg(target_os = "macos")]
const PNG: &[u8] = include_bytes!("../../../assets/app-icon/dalica-1024.png");
#[cfg(not(target_os = "macos"))]
const PNG: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/org.dalica.Dalica.png");

/// The window icon, or `None` (logged) if the embedded PNG does not decode.
pub fn window_icon() -> Option<IconData> {
    match eframe::icon_data::from_png_bytes(PNG) {
        Ok(icon) => Some(icon),
        Err(e) => {
            log::warn!("dalica: the app icon does not decode: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use dalica_core::app_id::APP_ID;
    use egui::IconData;

    fn dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets").join("app-icon")
    }

    fn read(rel: &[&str]) -> Vec<u8> {
        let path = rel.iter().fold(dir(), |p, part| p.join(part));
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn decode(png: &[u8], what: &str) -> IconData {
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "{what} is not a PNG");
        eframe::icon_data::from_png_bytes(png).unwrap_or_else(|e| panic!("{what} does not decode: {e}"))
    }

    fn alpha(icon: &IconData, x: u32, y: u32) -> u8 {
        icon.rgba[((y * icon.width + x) * 4 + 3) as usize]
    }

    fn be32(b: &[u8], i: usize) -> usize {
        u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
    }

    fn le16(b: &[u8], i: usize) -> usize {
        u16::from_le_bytes([b[i], b[i + 1]]) as usize
    }

    fn le32(b: &[u8], i: usize) -> usize {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
    }

    #[test]
    fn the_window_icon_decodes() {
        let icon = super::window_icon().expect("the embedded icon decodes");
        let side = if cfg!(target_os = "macos") { 1024 } else { 256 };
        assert_eq!((icon.width, icon.height), (side, side));
        assert_eq!(icon.rgba.len(), (side * side * 4) as usize);
        assert_eq!(alpha(&icon, side / 2, side / 2), 255, "the middle of the tile is opaque");
    }

    /// The canonical SVG keeps the family's tile and its three colours (`docs/assets.md` §7), and
    /// the scalable theme icon is the same file.
    #[test]
    fn the_canonical_svg_is_the_512_tile_in_three_colours() {
        let svg = String::from_utf8(read(&["dalica.svg"])).expect("UTF-8");
        assert!(svg.contains(r#"viewBox="0 0 512 512""#));
        assert!(svg.contains(r#"<rect width="512" height="512" rx="112"/>"#), "the rounded tile clips the artwork");
        let mut colours: Vec<String> = svg
            .match_indices('#')
            .filter_map(|(i, _)| svg.get(i..i + 7))
            .filter(|c| c[1..].chars().all(|ch| ch.is_ascii_hexdigit()))
            .map(str::to_lowercase)
            .collect();
        colours.sort();
        colours.dedup();
        assert_eq!(colours, ["#0b0b0c", "#1fa38a", "#efe9dc"], "ink, the Dalica teal field and paper, nothing else");
        let scalable = read(&["hicolor", "scalable", "apps", &format!("{APP_ID}.svg")]);
        assert_eq!(scalable, svg.as_bytes(), "the scalable theme icon is the canonical SVG");
    }

    /// Every hicolor PNG exists at its size, decodes, and fills the whole tile (the colour field
    /// reaches the middle of each edge); the 1024 px render sits on Apple's grid (a transparent
    /// margin around an opaque body).
    #[test]
    fn every_derived_png_exists_at_its_size_and_decodes() {
        for s in [16u32, 24, 32, 48, 64, 128, 256, 512] {
            let name = format!("{s}x{s}");
            let icon = decode(&read(&["hicolor", &name, "apps", &format!("{APP_ID}.png")]), &name);
            assert_eq!((icon.width, icon.height), (s, s), "hicolor/{name}");
            assert_eq!(alpha(&icon, s / 2, 0), 255, "hicolor/{name} is full-tile");
            assert_eq!(alpha(&icon, s / 2, s / 2), 255, "hicolor/{name}");
            assert!(alpha(&icon, 0, 0) < 128, "hicolor/{name} has rounded corners");
        }
        let mac = decode(&read(&["dalica-1024.png"]), "dalica-1024.png");
        assert_eq!((mac.width, mac.height), (1024, 1024));
        // 62 of 636 units each side: about 100 px of margin, then the 824 px body.
        assert_eq!(alpha(&mac, 512, 90), 0, "the macOS render has Apple's margin");
        assert_eq!(alpha(&mac, 512, 110), 255, "the macOS body starts after the margin");
        assert_eq!(alpha(&mac, 512, 512), 255);
    }

    /// The `.ico` holds every size the Windows shell asks for, each entry a PNG of its size.
    #[test]
    fn the_ico_has_every_shell_size() {
        let ico = read(&["dalica.ico"]);
        assert_eq!((le16(&ico, 0), le16(&ico, 2)), (0, 1), "ICONDIR, type 1 (icon)");
        let n = le16(&ico, 4);
        let mut sizes = Vec::new();
        for i in 0..n {
            let e = 6 + i * 16;
            let w = if ico[e] == 0 { 256 } else { u32::from(ico[e]) };
            let h = if ico[e + 1] == 0 { 256 } else { u32::from(ico[e + 1]) };
            assert_eq!(w, h);
            let (len, off) = (le32(&ico, e + 8), le32(&ico, e + 12));
            let png = ico.get(off..off + len).expect("the image lies inside the file");
            let icon = decode(png, &format!("ico entry {w}"));
            assert_eq!((icon.width, icon.height), (w, w), "ico entry {w} holds a {}px image", icon.width);
            sizes.push(w);
        }
        assert_eq!(sizes, [16, 20, 24, 32, 40, 48, 64, 128, 256]);
    }

    /// The `.icns` is well formed and holds every size of an iconset, 16 to 1024 px, each a PNG of
    /// the size its type says.
    #[test]
    fn the_icns_has_every_iconset_size() {
        let icns = read(&["dalica.icns"]);
        assert_eq!(icns.get(..4), Some(&b"icns"[..]));
        assert_eq!(be32(&icns, 4), icns.len(), "the header holds the file's length");
        let mut at = 8;
        let mut kinds = Vec::new();
        while at < icns.len() {
            let kind = String::from_utf8_lossy(&icns[at..at + 4]).into_owned();
            let len = be32(&icns, at + 4);
            assert!(len > 8, "{kind}");
            let png = icns.get(at + 8..at + len).expect("the entry lies inside the file");
            let side = match kind.as_str() {
                "icp4" => 16,
                "icp5" | "ic11" => 32,
                "icp6" | "ic12" => 64,
                "ic07" => 128,
                "ic08" | "ic13" => 256,
                "ic09" | "ic14" => 512,
                "ic10" => 1024,
                other => panic!("unexpected icns entry {other}"),
            };
            let icon = decode(png, &kind);
            assert_eq!((icon.width, icon.height), (side, side), "{kind}");
            kinds.push(kind);
            at += len;
        }
        assert_eq!(at, icns.len());
        assert_eq!(kinds, ["icp4", "icp5", "ic11", "icp6", "ic12", "ic07", "ic08", "ic13", "ic09", "ic14", "ic10"]);
    }
}
