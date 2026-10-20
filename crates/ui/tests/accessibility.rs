//! The window's AccessKit tree (M15.29, `ui-layout.md` → Keyboard and screen readers), headless with
//! AccessKit on, read from the tree update egui hands a screen reader: in the library, the edit view
//! on a photo and on a clip, a slideshow, the photo viewer, the Import and Collage rooms and every
//! dialog, no interactive node lacks a name and every slider has its value and range; the regions
//! are named landmarks (the tree, the light table, the side pane, the edit view, the rooms); the
//! light table's visible cells are list items with their state inside a named group per section;
//! the tree's rows say their level and count; the canvases are named images; every dialog is a
//! dialog with its title; at 100 000 photos the tree holds only the visible cells, and the reshape
//! costs little (`DALICA_PROFILE=1` prints the frame times with AccessKit on and off).
//!
//! The audit (what counts as interactive, the name and slider checks, one pass per view and per
//! dialog) is adapted from LightCraft `crates/ui-egui/tests/accessibility.rs` at revision d999e382
//! (<https://github.com/storytold/lightcraft>, MIT OR Apache-2.0; `NOTICE`). Changes for Dalica: the
//! tree is read straight from egui's `TreeUpdate` after a frame of the app run here rather than
//! through egui_kittest; Dalica's views, rooms and dialog kinds; the landmarks, cells, rows and
//! canvases checked too; a 100 000-photo library.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, demo_open, new_driver, put_jpeg};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_engine::synthetic::{self, Spec};
use dalica_ui::state::{Dialog, DialogKind, View};
use dalica_ui::{App, Driver};
use egui::accesskit::{Action, Node, NodeId, Role, TreeUpdate};
use egui::{Event, Pos2, Rect, vec2};
use serde_json::json;

// --- Running a frame with AccessKit on -------------------------------------------------------------

/// Runs one frame of the app in `d`'s context outside the driver (whose step keeps no tree), with
/// `events` as the frame's input (`Event::ModifiersChanged` for the modifiers). AccessKit is turned
/// on for it and stays on.
pub fn frame(d: &mut Driver, events: Vec<Event>) -> egui::FullOutput {
    let ctx = d.ctx().clone();
    ctx.enable_accesskit();
    let (w, h, native) = d.size();
    let mut input = dalica_ui_kit::raw_input(w, h, native);
    let ppp = native * ctx.zoom_factor();
    input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(w as f32 / ppp, h as f32 / ppp)));
    // egui moves its clock on by a frame.
    input.time = None;
    input.events = events;
    let mut out = ctx.run_ui(input, |ui| d.app_mut().frame(ui));
    // Nothing is painted from this frame.
    out.textures_delta.clear();
    out
}

/// The AccessKit tree of the next frame (two frames: the first one, with AccessKit just on, builds
/// what the second one reports).
pub fn tree(d: &mut Driver) -> Tree {
    frame(d, Vec::new());
    let out = frame(d, Vec::new());
    Tree::new(out.platform_output.accesskit_update.expect("AccessKit is on"))
}

/// A tree update with its parents and an index.
pub struct Tree {
    pub update: TreeUpdate,
    index: HashMap<NodeId, usize>,
    parent: HashMap<NodeId, NodeId>,
}

impl Tree {
    fn new(update: TreeUpdate) -> Tree {
        let index = update.nodes.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
        let parent = update.nodes.iter().flat_map(|(id, n)| n.children().iter().map(move |c| (*c, *id))).collect();
        Tree { update, index, parent }
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.index.get(&id).and_then(|i| self.update.nodes.get(*i)).map(|(_, n)| n)
    }

    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.update.nodes.iter().map(|(id, n)| (*id, n))
    }

    /// The node of the widget registered as `widget` in the last frame.
    pub fn widget(&self, d: &Driver, widget: &str) -> Option<(NodeId, &Node)> {
        let id = d.widgets().into_iter().find(|w| w.id == widget)?.egui_id?.accesskit_id();
        Some((id, self.get(id)?))
    }

    /// The nodes from `id` up to the root.
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut at = self.parent.get(&id).copied();
        while let Some(p) = at {
            if out.contains(&p) || out.len() > 256 {
                break;
            }
            out.push(p);
            at = self.parent.get(&p).copied();
        }
        out
    }

    /// The nearest ancestor of `id` with `role`.
    pub fn ancestor(&self, id: NodeId, role: Role) -> Option<&Node> {
        self.ancestors(id).into_iter().filter_map(|a| self.get(a)).find(|n| n.role() == role)
    }

    /// The nodes with `role` named `label`.
    pub fn named(&self, role: Role, label: &str) -> Vec<NodeId> {
        self.nodes().filter(|(_, n)| n.role() == role && n.label() == Some(label)).map(|(id, _)| id).collect()
    }

    /// Whether `inner` is below `outer`.
    pub fn within(&self, inner: NodeId, outer: NodeId) -> bool {
        self.ancestors(inner).contains(&outer)
    }

    /// Every node has one parent and is reached from the root.
    pub fn check_shape(&self) {
        let mut parents: HashMap<NodeId, usize> = HashMap::new();
        for (_, n) in self.nodes() {
            for c in n.children() {
                *parents.entry(*c).or_default() += 1;
                assert!(self.index.contains_key(c), "a child {c:?} is not in the tree");
            }
        }
        let twice: Vec<_> = parents.iter().filter(|(_, k)| **k > 1).collect();
        assert!(twice.is_empty(), "nodes with two parents: {twice:?}");
        let root = self.update.tree.as_ref().map(|t| t.root).unwrap();
        for (id, _) in self.nodes() {
            assert!(id == root || self.ancestors(id).last() == Some(&root), "{id:?} is not reached from the root");
        }
    }
}

// --- The audit -----------------------------------------------------------------------------------

/// A role a screen reader treats as a control.
pub fn interactive(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::Switch
            | Role::RadioButton
            | Role::Slider
            | Role::SpinButton
            | Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::ComboBox
            | Role::EditableComboBox
            | Role::Tab
            | Role::Link
            | Role::MenuItem
            | Role::MenuItemCheckBox
            | Role::MenuItemRadio
            | Role::ListBoxOption
            | Role::TreeItem
    )
}

/// Every problem found in `tree`, one line each: an interactive node with no name, a slider without
/// its value and range. Panics when the tree has hardly any controls (the view did not draw).
fn audit(d: &Driver, tree: &Tree, place: &str) -> Vec<String> {
    let names: HashMap<NodeId, String> = d.widgets().into_iter().filter_map(|w| Some((w.egui_id?.accesskit_id(), w.id))).collect();
    let mut problems = Vec::new();
    let mut count = 0;
    for (id, n) in tree.nodes() {
        let role = n.role();
        // egui's own scroll bars are announced by their role, as native ones are.
        if role == Role::ScrollBar {
            continue;
        }
        // A widget that takes the focus or a click is interactive whatever its role.
        let acts = |a| n.supports_action(a);
        if !interactive(role) && !acts(Action::Focus) && !acts(Action::Click) {
            continue;
        }
        count += 1;
        let name = n.label().filter(|l| !l.trim().is_empty()).or(n.value()).unwrap_or_default();
        let who = names.get(&id).cloned().unwrap_or_else(|| format!("{id:?}"));
        if name.trim().is_empty() {
            problems.push(format!("{place}: {who}: unnamed {role:?} at {:?}", n.bounds()));
        }
        if role == Role::Slider {
            match (n.numeric_value(), n.min_numeric_value(), n.max_numeric_value()) {
                (Some(v), Some(lo), Some(hi)) if lo <= v && v <= hi => {}
                (v, lo, hi) => problems.push(format!("{place}: {who}: slider {name:?} lacks its value and range ({v:?} in {lo:?} … {hi:?})")),
            }
        }
    }
    assert!(count > 5, "{place}: the tree has its controls ({count})");
    problems
}

// --- Fixtures ------------------------------------------------------------------------------------

/// The demo library (three folders of twenty) at 1400 × 900, settled.
fn demo(dir: &Path) -> Driver {
    let session = demo_open(dir, Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(App::new(Some(session)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

/// A 2 s MP4 of the boxes the probe reads (`mvhd`, one AVC track), no frames: enough for the
/// library to list a clip and the edit view to open it (as `video_tiles.rs` builds one).
fn clip() -> Vec<u8> {
    let times = |timescale: u32, duration: u32| [0u32, 0, 0, timescale, duration].iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<u8>>();
    let matrix: Vec<u8> = [0x1_0000i32, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000].iter().flat_map(|v| v.to_be_bytes()).collect();
    let mut mvhd = times(1000, 2000);
    mvhd.extend(0x1_0000u32.to_be_bytes());
    mvhd.extend(0x100u16.to_be_bytes());
    mvhd.extend([0; 10]);
    mvhd.extend(&matrix);
    mvhd.extend([0; 24]);
    mvhd.extend(2u32.to_be_bytes());
    let mut tkhd = vec![0, 0, 0, 7];
    tkhd.extend([0; 8]);
    tkhd.extend(1u32.to_be_bytes());
    tkhd.extend([0; 24]);
    tkhd.extend(&matrix);
    tkhd.extend((640u32 << 16).to_be_bytes());
    tkhd.extend((360u32 << 16).to_be_bytes());
    let mut entry = vec![0; 6];
    entry.extend(1u16.to_be_bytes());
    entry.extend([0; 16]);
    entry.extend(640u16.to_be_bytes());
    entry.extend(360u16.to_be_bytes());
    entry.extend([0; 50]);
    let mut stsd = vec![0; 4];
    stsd.extend(1u32.to_be_bytes());
    stsd.extend(bx(b"avc1", &entry));
    let mut stsz = vec![0; 4];
    stsz.extend(1000u32.to_be_bytes());
    stsz.extend(60u32.to_be_bytes());
    let stbl = bx(b"stbl", &[bx(b"stsd", &stsd), bx(b"stsz", &stsz)].concat());
    let mut mdhd = times(30_000, 60_000);
    mdhd.extend([0; 4]);
    let mut hdlr = vec![0; 8];
    hdlr.extend(b"vide");
    hdlr.extend([0; 13]);
    let mdia = bx(b"mdia", &[bx(b"mdhd", &mdhd), bx(b"hdlr", &hdlr), bx(b"minf", &stbl)].concat());
    let moov = bx(b"moov", &[bx(b"mvhd", &mvhd), bx(b"trak", &[bx(b"tkhd", &tkhd), mdia].concat())].concat());
    let mut ftyp = b"mp42".to_vec();
    ftyp.extend(0x200u32.to_be_bytes());
    ftyp.extend(b"mp42isom");
    [bx(b"ftyp", &ftyp), bx(b"mdat", &[0x5a; 256]), moov].concat()
}

/// `Trip` with two photos and a clip, the first photo starred and captioned at length.
fn trip(dir: &Path) -> (Driver, [PhotoId; 3]) {
    let trip = dir.join("Trip");
    put_jpeg(&trip.join("a.jpg"), 21);
    put_jpeg(&trip.join("b.jpg"), 22);
    std::fs::write(trip.join("clip.mp4"), clip()).unwrap();
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": trip })).unwrap();
    s.wait_idle();
    let id = |name: &str| s.library().photos().find(|p| p.name == name).unwrap().id;
    let ids = [id("a.jpg"), id("b.jpg"), id("clip.mp4")];
    s.execute("photo.star.toggle", &json!({ "ids": [ids[0].get()] })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [ids[0].get()], "caption": CAPTION })).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.run("view.caption", &json!({ "caption": "caption" })).unwrap();
    d.settle(FRAMES).unwrap();
    (d, ids)
}

/// Selects the library's first four photos and opens the Collage room on them.
fn collage(d: &mut Driver) {
    let ids: Vec<u64> = d.app().session().unwrap().library().photos().take(4).map(|p| p.id.get()).collect();
    d.run("select.set", &json!({ "ids": ids })).unwrap();
    d.run("create.collage", &json!({ "ids": ids })).unwrap();
    d.settle(FRAMES).unwrap();
}

const CAPTION: &str = "The harbour at dawn, with the fishing boats coming back in after a long night out at sea";

// --- The audit over every view and dialog --------------------------------------------------------

#[test]
fn no_interactive_node_lacks_a_name_and_every_slider_has_its_value_and_range_in_every_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    let mut problems = Vec::new();
    let mut audit_now = |d: &mut Driver, place: &str| {
        d.settle(FRAMES).unwrap();
        let t = tree(d);
        t.check_shape();
        problems.extend(audit(d, &t, place));
    };
    audit_now(&mut d, "library");
    for pane in ["pane.properties", "pane.tags", "pane.people", "pane.places"] {
        d.run(pane, &json!({})).unwrap();
        audit_now(&mut d, pane);
    }
    d.run("pane.toggle", &json!({})).unwrap();
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    audit_now(&mut d, "edit view");
    for tab in ["tuning", "effects", "effects2", "effects3"] {
        d.run("edit.tab", &json!({ "tab": tab })).unwrap();
        audit_now(&mut d, &format!("edit view, {tab}"));
    }
    d.run("edit.tab", &json!({ "tab": "basic" })).unwrap();
    for tool in ["crop", "straighten", "retouch"] {
        d.run("edit.tool.open", &json!({ "tool": tool })).unwrap();
        audit_now(&mut d, &format!("edit view, {tool}"));
        d.key("Esc").unwrap();
    }
    d.run("view.library", &json!({})).unwrap();
    d.run("view.slideshow", &json!({})).unwrap();
    audit_now(&mut d, "slideshow");
    d.key("Esc").unwrap();
    collage(&mut d);
    audit_now(&mut d, "collage room");
    d.run("import.open", &json!({})).unwrap();
    audit_now(&mut d, "import room");
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "{} accessibility problems:\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn the_edit_view_on_a_clip_and_the_photo_viewer_name_every_control() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, ids) = trip(dir.path());
    d.run("select.set", &json!({ "ids": [ids[2].get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().edit.photo, Some(ids[2]));
    let t = tree(&mut d);
    let mut problems = audit(&d, &t, "edit view on a clip");

    // The viewer: a session opened on a file.
    let folder = dir.path().join("Trip");
    let mut s = Session::open(&dir.path().join("viewer-library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("options.set", &json!({ "values": { "viewer.hideControls": false } })).unwrap();
    s.open_viewer(&folder.join("a.jpg")).unwrap();
    let mut v = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    v.settle(FRAMES).unwrap();
    assert_eq!(v.state().view, View::Viewer);
    let vt = tree(&mut v);
    vt.check_shape();
    problems.extend(audit(&v, &vt, "photo viewer"));
    let (canvas, node) = vt.widget(&v, "canvas:viewer").expect("the viewer's photo");
    assert_eq!((node.role(), node.label()), (Role::Image, Some("a.jpg")));
    let main = vt.named(Role::Main, "Photo viewer");
    assert!(main.len() == 1 && vt.within(canvas, main[0]), "the viewer is one main landmark round its photo");
    assert!(problems.is_empty(), "{} accessibility problems:\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn every_dialog_is_a_titled_dialog_whose_controls_are_named() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    let mut problems = Vec::new();
    let mut opened = 0;
    for kind in DialogKind::ALL {
        d.app_mut().state_mut().dialogs.clear();
        if let Err(e) = dalica_ui::dialogs::open(d.app_mut(), Dialog::new(kind)) {
            problems.push(format!("{kind:?}: does not open ({e})"));
            continue;
        }
        d.settle(FRAMES).unwrap();
        if d.state().dialogs.last().map(|x| x.kind) != Some(kind) {
            continue; // it closed itself: nothing to show without its context
        }
        opened += 1;
        let t = tree(&mut d);
        t.check_shape();
        problems.extend(audit(&d, &t, &format!("{kind:?}")));
        let dialogs: Vec<&Node> = t.nodes().map(|(_, n)| n).filter(|n| n.role() == Role::Dialog).collect();
        if dialogs.is_empty() || dialogs.iter().any(|n| n.label().is_none_or(|l| l.trim().is_empty())) {
            problems.push(format!("{kind:?}: {} dialog nodes, some without a title", dialogs.len()));
        }
    }
    d.app_mut().state_mut().dialogs.clear();
    assert!(opened >= DialogKind::ALL.len() - 4, "the dialogs drew ({opened} of {})", DialogKind::ALL.len());
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "{} accessibility problems:\n{}", problems.len(), problems.join("\n"));
}

// --- Landmarks, cells, rows and canvases ---------------------------------------------------------

#[test]
fn the_regions_are_named_landmarks_round_their_controls() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    d.run("pane.properties", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let t = tree(&mut d);
    t.check_shape();
    let one = |role: Role, label: &str| -> NodeId {
        let found = t.named(role, label);
        assert_eq!(found.len(), 1, "one {role:?} named {label:?}: {found:?}");
        found[0]
    };
    let inside = |widget: &str, landmark: NodeId| {
        let (id, _) = t.widget(&d, widget).unwrap_or_else(|| panic!("{widget} in the tree"));
        assert!(t.within(id, landmark), "{widget} is inside its landmark");
    };
    let tree_pane = one(Role::Navigation, "Library tree");
    inside("tree:folder:1", tree_pane);
    inside("tree:group:folders", tree_pane);
    let table = one(Role::Main, "Light table");
    inside("thumb:1", table);
    inside("scroll:down", table);
    inside("button:pane.close", one(Role::Complementary, "Properties"));
    inside("menu:File", one(Role::MenuBar, "Menu bar"));
    inside("button:toolbar.import", one(Role::Toolbar, "Toolbar"));
    inside("action:export", one(Role::Toolbar, "Actions"));

    // The edit view: its photo a named image inside the main landmark.
    let photo = d.app().session().unwrap().library().photos().next().unwrap().clone();
    d.run("select.set", &json!({ "ids": [photo.id.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    d.run("edit.tool.open", &json!({ "tool": "crop" })).unwrap();
    d.settle(FRAMES).unwrap();
    let t = tree(&mut d);
    t.check_shape();
    let edit = t.named(Role::Main, "Edit view");
    let (canvas, node) = t.widget(&d, "canvas:edit").unwrap();
    assert_eq!((node.role(), node.label()), (Role::Image, Some(photo.name.as_str())));
    assert_eq!((node.value(), node.description()), (Some("Fit to the window"), Some("crop tool open")));
    assert!(edit.len() == 1 && t.within(canvas, edit[0]), "the photo is in the edit view's landmark");
    let (tab, _) = t.widget(&d, "tab:edit.basic").unwrap();
    assert!(t.ancestor(tab, Role::Region).is_some_and(|n| n.label() == Some("Edit panel")));
    d.key("Esc").unwrap();

    // The rooms: each the main landmark, named after the room, the collage its named image.
    d.run("view.library", &json!({})).unwrap();
    collage(&mut d);
    d.settle(FRAMES).unwrap();
    let t = tree(&mut d);
    let (canvas, node) = t.widget(&d, "canvas:collage").expect("the collage canvas");
    assert_eq!(node.role(), Role::Image);
    assert_eq!(node.label(), Some("Collage"));
    let room = t.named(Role::Main, "Collage");
    assert!(room.len() == 1 && t.within(canvas, room[0]));
    assert_eq!(t.named(Role::TabList, "Rooms").len(), 1, "the room tabs");
    d.run("import.open", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let t = tree(&mut d);
    assert_eq!(t.named(Role::Main, "Import").len(), 1, "the Import room");
}

#[test]
fn the_light_tables_cells_are_list_items_with_their_state_in_a_group_per_section() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, ids) = trip(dir.path());
    d.run("select.set", &json!({ "ids": [ids[0].get()] })).unwrap();
    d.settle(FRAMES).unwrap();
    let t = tree(&mut d);
    t.check_shape();
    let cell = |id: PhotoId| t.widget(&d, &format!("thumb:{}", id.get())).unwrap_or_else(|| panic!("{id} drawn"));
    let (a, a_node) = cell(ids[0]);
    assert_eq!((a_node.role(), a_node.label(), a_node.is_selected()), (Role::ListBoxOption, Some("a.jpg"), Some(true)));
    let description = a_node.description().unwrap_or_default();
    assert!(description.starts_with("Starred"), "{description}");
    assert!(description.ends_with(&format!("Caption: {CAPTION}")), "the whole caption, though the cell elides it: {description}");
    let (_, b_node) = cell(ids[1]);
    assert_eq!((b_node.is_selected(), b_node.description()), (Some(false), None));
    let (clip, clip_node) = cell(ids[2]);
    assert_eq!(clip_node.description(), Some("Video, 0:02"));
    // The section's group, named after its title, inside the light table.
    let group = t.ancestor(a, Role::Group).expect("a section group");
    assert_eq!(group.label(), Some("Trip"));
    let groups = t.named(Role::Group, "Trip");
    assert!(groups.len() == 1 && t.within(clip, groups[0]), "every cell of the section in one group");
    let table = t.named(Role::Main, "Light table");
    assert!(t.within(groups[0], table[0]));
    // The section's header buttons are in the group too.
    let (play, _) = t.widget(&d, &format!("section:{}/play", d.state().selection.section.unwrap())).expect("the header's Play");
    assert!(t.within(play, groups[0]));
}

#[test]
fn the_trees_rows_say_their_level_whether_they_are_open_and_their_count() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    let t = tree(&mut d);
    let (id, row) = t.widget(&d, "tree:folder:1").unwrap();
    assert_eq!((row.role(), row.level(), row.is_expanded()), (Role::TreeItem, Some(1), None));
    assert_eq!(row.description(), Some("20 photos"));
    // Inside its group's tree, with its place among its siblings (the rows out of view are not in
    // the tree, so a screen reader learns the set's size from the row).
    let folders = t.ancestor(id, Role::Tree).expect("the row is in a tree");
    assert_eq!(folders.label(), Some("Folders"));
    let (position, size) = (row.position_in_set().unwrap(), row.size_of_set().unwrap());
    let count = d.app().session().unwrap().library().folders().count();
    assert!(position >= 1 && position <= size && size == count, "{position} of {size}, {count} folders");

    // In tree mode a folder with folders below it says whether it is open, each at its depth.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("photos");
    put_jpeg(&root.join("Trip").join("a.jpg"), 1);
    put_jpeg(&root.join("Trip").join("Day 2").join("b.jpg"), 2);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.click("button:tree.tree").unwrap();
    d.settle(FRAMES).unwrap();
    let t = tree(&mut d);
    let mut rows: Vec<(String, usize, Option<bool>)> = t
        .nodes()
        .filter(|(_, n)| n.role() == Role::TreeItem)
        .filter_map(|(_, n)| Some((n.label()?.to_owned(), n.level()?, n.is_expanded())))
        .collect();
    rows.sort_by_key(|r| r.1);
    let open: Vec<&(String, usize, Option<bool>)> = rows.iter().filter(|r| r.2 == Some(true)).collect();
    assert!(!open.is_empty(), "a folder with folders below it is open: {rows:?}");
    assert!(rows.iter().any(|r| r.0.starts_with("Day 2") && r.1 > open[0].1 && r.2.is_none()), "Day 2 one level down: {rows:?}");
}

#[test]
fn at_100000_photos_the_tree_holds_only_the_visible_cells() {
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec::new(100_000, 1_000, synthetic::SEED);
    let s = synthetic::open(dir.path(), &spec, Arc::new(dalica_core::SystemClock)).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    // Frame times with AccessKit off, then on (the reshape included).
    let time = |d: &mut Driver, on: bool, n: usize| -> Duration {
        let mut times: Vec<Duration> = (0..n)
            .map(|_| {
                let started = Instant::now();
                if on {
                    frame(d, Vec::new());
                } else {
                    d.step();
                }
                started.elapsed()
            })
            .collect();
        times.sort();
        times[times.len() / 2]
    };
    let off = time(&mut d, false, 20);
    let t = tree(&mut d);
    let on = time(&mut d, true, 20);
    let cells = t.nodes().filter(|(_, n)| n.role() == Role::ListBoxOption).count();
    let drawn = d.widgets().iter().filter(|w| w.role == dalica_ui_kit::registry::Role::Thumbnail && w.egui_id.is_some()).count();
    assert!(cells > 0 && cells == drawn, "the tree holds the drawn cells: {cells} of {drawn}");
    assert!(cells < 200, "a screenful, not the library: {cells}");
    assert!(t.update.nodes.len() < 2_000, "{} nodes", t.update.nodes.len());
    t.check_shape();
    if std::env::var_os("DALICA_PROFILE").is_some() {
        eprintln!("AccessKit at 100 000 photos: {} nodes, {cells} cells; median frame {off:?} off, {on:?} on", t.update.nodes.len());
    }
}
