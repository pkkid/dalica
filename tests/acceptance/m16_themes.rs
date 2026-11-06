//! M16's *Themes* in the live app (`m16.rs` part 1): `dalica --headless` on a folder of eight
//! painted photos, driven over its control channel, with `dalica-cli run --connect` and
//! `dalica-cli mcp --connect` beside it on the same app (`m16_mcp.rs`). Each of the fifteen themes
//! is chosen from View ▸ Theme ▸ (its row checked after), then by `dalica-cli` over the control
//! channel, then by `cmd_theme_<id>` over MCP; each time the very next frame (`ui.screenshot
//! {settle: false}`) is painted in it: the toolbar paints the theme's `chrome` most, the light
//! table its `grid_bg`, the tray its `tray_bg` (`Tokens::for_kind`), and `ui.state` → `theme`
//! names it. The last theme chosen, Pro (Light) over MCP, is the one a restart opens in. Options ▸
//! General ▸ Interface ▸ Theme previews Studio (Dark) in the next frame without saving it, and
//! Cancel puts the saved theme back. Whole-window fingerprints of the library in Dark, Pro (Light)
//! and Classic (`fingerprints/m16-1400x900-<id>.json`).

use std::collections::HashMap;
use std::path::Path;

use dalica_ui_kit::Tokens;
use dalica_ui_kit::theme::ThemeKind;
use egui::Color32;
use serde_json::{Value, json};

use crate::app::{self, idle, run};
use crate::common::{self, COLOURS, LARGE, steady_shot};
use crate::live::{self, Client};
use crate::mcp::{self, Mcp};

/// A region by widget id, the token it paints most, and that token.
type Expect = (&'static str, &'static str, fn(&Tokens) -> Color32);

/// The library's regions whose colour says which theme the frame wears (as `crates/ui/tests/themes.rs`
/// checks them in every theme).
const LIBRARY: [Expect; 3] =
    [("region:toolbar", "chrome", |t| t.chrome), ("region:lightTable", "grid_bg", |t| t.grid_bg), ("region:tray", "tray_bg", |t| t.tray_bg)];

/// A whole-window frame: its width and RGBA pixels.
struct Frame {
    width: u32,
    rgba: Vec<u8>,
}

fn decode(bytes: &[u8]) -> Frame {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.color_type, info.bit_depth), (png::ColorType::Rgba, png::BitDepth::Eight));
    buf.truncate(info.buffer_size());
    assert_eq!((info.width, info.height), LARGE);
    Frame { width: info.width, rgba: buf }
}

/// The path a frame of `label` is written to (inside the app's temporary folder, where automation
/// may write a screenshot).
fn frame_path(home: &Path, label: &str) -> std::path::PathBuf {
    home.join("tmp").join(format!("m16-{label}.png"))
}

/// The next frame as it is (`ui.screenshot {settle: false}`), over the control connection.
fn next_frame(c: &mut Client, home: &Path, label: &str) -> Frame {
    let path = frame_path(home, label);
    c.ok("ui.screenshot", json!({ "path": path, "settle": false }));
    decode(&std::fs::read(&path).unwrap())
}

/// The colour `rect` (points, at scale 1 the frame's pixels) paints most.
fn most(f: &Frame, rect: [f64; 4]) -> Color32 {
    let height = f.rgba.len() as u32 / 4 / f.width;
    let (x0, y0) = (rect[0].max(0.0) as u32, rect[1].max(0.0) as u32);
    let (x1, y1) = (((rect[0] + rect[2]) as u32).min(f.width), ((rect[1] + rect[3]) as u32).min(height));
    let mut counts: HashMap<[u8; 4], usize> = HashMap::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * f.width + x) * 4) as usize;
            *counts.entry([f.rgba[i], f.rgba[i + 1], f.rgba[i + 2], f.rgba[i + 3]]).or_default() += 1;
        }
    }
    let ([r, g, b, a], _) = counts.into_iter().max_by_key(|(c, n)| (*n, *c)).unwrap_or_else(|| panic!("an empty rect {rect:?}"));
    Color32::from_rgba_premultiplied(r, g, b, a)
}

/// The rects of the regions of `expect`, by widget id.
fn rects(c: &mut Client, expect: &[Expect]) -> Vec<[f64; 4]> {
    expect.iter().map(|(id, _, _)| app::rect_of(c, id).unwrap_or_else(|| panic!("{id} is drawn"))).collect()
}

/// Checks that `frame` wears `kind`: each region of `expect` paints its token most.
fn wears(frame: &Frame, kind: ThemeKind, expect: &[Expect], at: &[[f64; 4]], how: &str) {
    let t = Tokens::for_kind(kind);
    for ((id, token, of), rect) in expect.iter().zip(at) {
        let got = most(frame, *rect);
        assert_eq!(got, of(&t), "{} by {how}: {id} paints {got:?} in the next frame, not `{token}`", kind.id());
    }
}

/// The ids of the View ▸ Theme ▸ rows that are checked.
fn checked(c: &mut Client) -> Vec<String> {
    let tree = c.ok("ui.menu.tree", json!({}));
    let view = tree.as_array().unwrap().iter().find(|m| m["widget"] == "menu:View").cloned().unwrap();
    let theme = view["children"].as_array().unwrap().iter().find(|n| n["label"] == "Theme").cloned().unwrap();
    theme["children"].as_array().unwrap().iter().filter(|n| n["checked"] == true).map(|n| n["id"].as_str().unwrap().to_string()).collect()
}

fn theme_of(c: &mut Client) -> String {
    c.state()["theme"].as_str().unwrap_or_default().to_string()
}

/// The themes in the order a sweep chooses them: every one after Dark, then Dark, so each choice
/// changes the theme.
fn sweep() -> Vec<ThemeKind> {
    ThemeKind::ALL.iter().skip(1).chain(ThemeKind::ALL.first()).copied().collect()
}

/// A folder of eight painted photos, with fixed times, in `home/Garden`.
fn garden(home: &Path) -> std::path::PathBuf {
    let garden = home.join("Garden");
    std::fs::create_dir_all(&garden).unwrap();
    for (i, colour) in COLOURS.iter().enumerate() {
        app::put(&garden.join(format!("IMG_{:04}.jpg", i + 1)), &common::painted_jpeg(320, 240, *colour), i as u64);
    }
    garden
}

pub fn themes(exe: &Path, home: &Path) {
    let cli = mcp::cli_exe();
    let garden = garden(home);
    let (a, mut c) = live::start(exe, home);
    run(&mut c, "folders.add", json!({ "path": garden }));
    idle(&mut c);
    assert_eq!(theme_of(&mut c), "dark", "Dark by default");
    assert_eq!(checked(&mut c), ["theme.dark"]);
    app::park(&mut c, f64::from(LARGE.1));
    steady_shot(&mut c, home, "m16-1400x900-dark", &[]);
    let at = rects(&mut c, &LIBRARY);
    wears(&next_frame(&mut c, home, "dark"), ThemeKind::Dark, &LIBRARY, &at, "default");

    // From View ▸ Theme ▸, each theme in turn: the next frame wears it and its row is checked.
    for kind in sweep() {
        let id = kind.id();
        app::click(&mut c, "menu:View");
        app::click(&mut c, "submenu:View/Theme");
        c.ok("ui.clickWidget", json!({ "id": format!("menuitem:theme.{id}") }));
        wears(&next_frame(&mut c, home, &format!("menu-{id}")), kind, &LIBRARY, &at, "View ▸ Theme ▸");
        assert_eq!(theme_of(&mut c), id);
        assert_eq!(checked(&mut c), [format!("theme.{id}")], "its row is checked");
    }
    println!("  View ▸ Theme ▸: each of {} themes worn in the next frame, its row checked", ThemeKind::ALL.len());

    // By `dalica-cli run --connect` over the control channel: the command, then the next frame.
    for kind in sweep() {
        let id = kind.id();
        let path = frame_path(home, &format!("cli-{id}"));
        let out = mcp::run(
            &cli,
            home,
            &[
                json!({ "command": format!("theme.{id}") }),
                json!({ "method": "ui.screenshot", "params": { "path": path, "settle": false } }),
                json!({ "method": "ui.state" }),
            ],
        );
        assert_eq!(out[0]["result"], json!({ "theme": id, "changed": true }), "{id}");
        wears(&decode(&std::fs::read(&path).unwrap()), kind, &LIBRARY, &at, "dalica-cli");
        assert_eq!(out[2]["result"]["theme"], id);
    }
    println!("  dalica-cli run --connect: each theme worn in the next frame");

    // By `cmd_theme_<id>` over `dalica-cli mcp --connect`; Pro (Light) last.
    let mut m = Mcp::connect(&cli, home);
    let tools = m.tools();
    for kind in ThemeKind::ALL {
        assert!(tools.contains(&format!("cmd_theme_{}", kind.id())), "cmd_theme_{} is listed", kind.id());
    }
    let mut order = sweep();
    order.push(ThemeKind::ProLight);
    for kind in order {
        let id = kind.id();
        let r = m.tool(&format!("cmd_theme_{id}"), json!({}));
        assert_eq!((&r["theme"], &r["changed"]), (&json!(id), &json!(true)), "{r}");
        let path = frame_path(home, &format!("mcp-{id}"));
        m.tool("control_call", json!({ "method": "ui.screenshot", "params": { "path": path, "settle": false } }));
        wears(&decode(&std::fs::read(&path).unwrap()), kind, &LIBRARY, &at, "MCP");
        let state = m.tool("control_call", json!({ "method": "ui.state", "params": {} }));
        assert_eq!(state["theme"], id, "{state}");
    }
    m.finish();
    println!("  dalica-cli mcp --connect: each theme worn in the next frame, Pro (Light) last");
    live::quit(a, c);

    // A restart opens in the last theme chosen.
    let (a, mut c) = live::start(exe, home);
    idle(&mut c);
    assert_eq!(theme_of(&mut c), "proLight", "a restart opens in Pro (Light)");
    assert_eq!(checked(&mut c), ["theme.proLight"]);
    app::park(&mut c, f64::from(LARGE.1));
    steady_shot(&mut c, home, "m16-1400x900-proLight", &[]);
    wears(&next_frame(&mut c, home, "restart"), ThemeKind::ProLight, &LIBRARY, &at, "a restart");
    println!("  a restart opens in Pro (Light)");

    // Options ▸ General ▸ Interface ▸ Theme: previewed in the next frame, Cancel reverts.
    run(&mut c, "app.preferences", json!({}));
    app::click(&mut c, "choice:option.interface.theme");
    c.ok("ui.clickWidget", json!({ "id": "choice:option.interface.theme/studio" }));
    let dialog: [Expect; 1] = [("dialog:options", "chrome", |t| t.chrome)];
    let frame = next_frame(&mut c, home, "options-preview");
    let dialog_at = rects(&mut c, &dialog);
    wears(&frame, ThemeKind::Studio, &dialog, &dialog_at, "the Options preview");
    let state = c.state();
    assert_eq!((&state["theme"], &state["options"]["previewTheme"]), (&json!("studio"), &json!("studio")), "{state}");
    assert_eq!(saved_theme(home), "proLight", "a preview is not saved");
    println!("  the Options row previews Studio (Dark) in the next frame, unsaved");
    app::click(&mut c, "button:dialog.cancel");
    assert!(c.state()["options"].is_null(), "Cancel closes the dialog");
    assert_eq!(theme_of(&mut c), "proLight", "Cancel reverts to the saved theme");
    wears(&next_frame(&mut c, home, "options-cancel"), ThemeKind::ProLight, &LIBRARY, &at, "Cancel");
    println!("  Cancel puts Pro (Light) back");

    // Classic, set as `ui.set {theme}` does.
    c.ok("ui.set", json!({ "theme": "classic" }));
    wears(&next_frame(&mut c, home, "classic"), ThemeKind::Classic, &LIBRARY, &at, "ui.set");
    app::park(&mut c, f64::from(LARGE.1));
    steady_shot(&mut c, home, "m16-1400x900-classic", &[]);
    live::quit(a, c);
}

/// `interface.theme` as `settings.json` in the library holds it.
fn saved_theme(home: &Path) -> String {
    let v: Value = serde_json::from_slice(&std::fs::read(home.join("library").join("settings.json")).unwrap()).unwrap();
    v["interface"]["theme"].as_str().unwrap_or_else(|| panic!("no interface.theme in {v}")).to_string()
}
