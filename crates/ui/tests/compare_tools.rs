//! The tools on A│A's right side (`editing.md` §4, M11.27), headless and by id: Crop, Straighten,
//! Retouch, the Tuning tab and the Basic Fixes Fill Light each build their draft and preview from
//! the variant's recipe and apply to the variant, leaving the photo's own recipe and its sidecar
//! byte-identical; no part of the panel is greyed there any more; "Keep which version?" → Right
//! then writes the tool's step as one step of the photo's history. A tool applied after the
//! variant went away fails gracefully and touches nothing. In A│B with the right side focused the
//! histogram is that photo's.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, new_driver, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_engine::variant::Keep;
use dalica_scenes::Scene;
use dalica_ui::panels::edit::histogram;
use dalica_ui::state::{Compare, Side, ToolKind};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// A library under `dir` holding one folder of two 900 × 600 photos, and the window in the edit
/// view on the first: the folder, the driver and the photos in name order.
fn editing(dir: &Path) -> (PathBuf, Driver, Vec<PhotoId>) {
    let folder = dir.join("photos").join("Harbour");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..2u64 {
        let bytes = dalica_scenes::jpeg(&Scene::new(70 + i, 900, 600), 90, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    let ids: Vec<PhotoId> = ids.into_iter().map(|(_, id)| id).collect();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    run(&mut d, "view.edit", json!({ "id": ids[0] }));
    (folder, d, ids)
}

fn settle(d: &mut Driver) {
    d.settle(FRAMES).unwrap();
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    settle(d);
}

fn run(d: &mut Driver, id: &str, params: Value) -> Value {
    let v = d.app_mut().run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    settle(d);
    v
}

fn action(d: &mut Driver, name: &str, params: Value) -> Value {
    run(d, "edit.tool.action", json!({ "name": name, "params": params }))
}

fn edits(d: &Driver, id: PhotoId) -> Vec<String> {
    d.app().session().unwrap().library().info(id).edits.clone()
}

fn variant(d: &Driver) -> Vec<String> {
    d.app().session().unwrap().variant().edits().map(<[String]>::to_vec).unwrap_or_default()
}

/// The folder's sidecar bytes after every pending write landed.
fn sidecar(d: &mut Driver, folder: &Path) -> Vec<u8> {
    d.app_mut().session_mut().unwrap().flush().unwrap();
    std::fs::read(folder.join(".dalica.json")).unwrap()
}

/// A│A with the right side (the variant) focused.
fn on_the_right(d: &mut Driver) {
    click(d, "button:compare.same");
    click(d, "canvas:edit.other");
    assert_eq!((d.state().edit.compare, d.state().edit.focus), (Compare::Same, Side::Right));
}

/// The recipe the focused side's preview shows now (a tool's candidate included).
fn shown(d: &Driver) -> Vec<String> {
    d.app().session().unwrap().preview().map(|p| p.recipe.clone()).unwrap_or_default()
}

/// A tool on the focused side: its draft made (`None`: a change taken at once) and then applied.
type Step = fn(&mut Driver);

fn crop_draft(d: &mut Driver) {
    click(d, "tool:crop");
    action(d, "draw", json!({ "from": [0.2, 0.2], "to": [0.7, 0.8] }));
    action(d, "preview", json!({ "on": true }));
}

fn straighten_draft(d: &mut Driver) {
    click(d, "tool:straighten");
    action(d, "set", json!({ "angle": 3.0 }));
}

fn retouch_draft(d: &mut Driver) {
    click(d, "tool:retouch");
    action(d, "patch", json!({ "target": [0.3, 0.6], "source": [0.45, 0.6] }));
}

fn apply(d: &mut Driver) {
    click(d, "button:panel.apply");
}

fn tuning_change(d: &mut Driver) {
    click(d, "tab:edit.tuning");
    assert!(widget(d, "slider:tuning.fill_light").enabled, "the Tuning tab is not greyed");
    run(d, "edit.tool.open", json!({ "tool": "tuning" }));
    action(d, "set", json!({ "fill": 0.4, "temperature": 0.25 }));
    run(d, "edit.tool.apply", json!({}));
    click(d, "tab:edit.basic");
}

fn fill_light_change(d: &mut Driver) {
    assert!(widget(d, "slider:basic.fill_light").enabled, "the Fill Light row is not greyed");
    click(d, "slider:basic.fill_light");
}

#[test]
fn each_tool_on_the_right_of_a_a_edits_the_variant_and_keeping_it_is_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, mut d, ids) = editing(dir.path());
    let photo = ids[0];
    // The photo has an edit of its own, so its sidecar exists before compare starts.
    click(&mut d, "tool:autocontrast");
    let tools: [(&str, &str, Option<Step>, Step); 5] = [
        ("crop", "crop", Some(crop_draft), apply),
        ("straighten", "straighten", Some(straighten_draft), apply),
        ("retouch", "retouch", Some(retouch_draft), apply),
        ("tuning", "tuning", None, tuning_change),
        ("fill light", "tuning", None, fill_light_change),
    ];
    for (name, op, draft, finish) in tools {
        let before = edits(&d, photo);
        let bytes = sidecar(&mut d, &folder);
        on_the_right(&mut d);
        assert!(d.widgets().iter().all(|w| !w.id.starts_with("label:edit.toolsLater")), "{name}: no wash over the tools");
        for id in ["tool:crop", "tool:straighten", "tool:retouch", "slider:basic.fill_light"] {
            assert!(widget(&d, id).enabled, "{name}: {id} is not greyed on the variant");
        }
        // The variant takes a step of its own first, so what the tool builds on tells the sides apart.
        click(&mut d, "tool:autofix");
        let base = variant(&d);
        assert_eq!(base.last().map(String::as_str), Some("autofix()"));
        if let Some(draft) = draft {
            draft(&mut d);
            assert!(d.state().edit.tool.is_some(), "{name}: the tool opened on the variant");
            assert!(shown(&d).iter().any(|e| e == "autofix()"), "{name}: the candidate is built on the variant's recipe: {:?}", shown(&d));
        }
        finish(&mut d);
        assert_eq!(d.state().edit.tool, None, "{name}: applied and closed");
        let right = variant(&d);
        assert_ne!(right, base, "{name}: the variant took the tool's step");
        assert!(right.last().is_some_and(|e| e.starts_with(&format!("{op}("))), "{name}: {right:?}");
        assert_eq!(edits(&d, photo), before, "{name}: the photo's own recipe is untouched");
        assert_eq!(sidecar(&mut d, &folder), bytes, "{name}: the sidecar is byte-identical");
        assert_eq!(shown(&d), right, "{name}: the right side shows the variant");

        // Right: the variant's recipe, the tool's step with it, is one step of the photo's history.
        click(&mut d, "button:compare.one");
        click(&mut d, "button:dialog.right");
        assert_eq!(d.state().edit.compare, Compare::One);
        assert_eq!(edits(&d, photo), right, "{name}: kept");
        let v: Value = serde_json::from_slice(&sidecar(&mut d, &folder)).unwrap();
        assert_eq!(v["photos"]["IMG_0001.jpg"]["edits"], json!(right), "{name}");
        run(&mut d, "edit.undo", json!({}));
        assert_eq!(edits(&d, photo), before, "{name}: one Undo gives the recipe before it back");
        run(&mut d, "edit.redo", json!({}));
    }
}

#[test]
fn a_tool_applied_after_the_variant_went_away_fails_and_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, mut d, ids) = editing(dir.path());
    let photo = ids[0];
    click(&mut d, "tool:autocontrast");
    let (before, bytes) = (edits(&d, photo), sidecar(&mut d, &folder));
    on_the_right(&mut d);
    for (tool, draft) in [("straighten", straighten_draft as Step), ("crop", crop_draft), ("retouch", retouch_draft)] {
        draft(&mut d);
        // The variant is dropped behind the window's back (a script keeping the left side).
        d.app_mut().session_mut().unwrap().close_variant(Keep::Left).unwrap();
        let r = d.app_mut().run("edit.tool.apply", &json!({}));
        assert!(r.is_err(), "{tool}: {r:?}");
        assert_eq!(d.state().edit.tool.map(ToolKind::name), Some(tool), "{tool}: the tool stays open to try again or cancel");
        run(&mut d, "edit.tool.cancel", json!({}));
        assert_eq!(edits(&d, photo), before, "{tool}: the photo is untouched");
        assert_eq!(sidecar(&mut d, &folder), bytes, "{tool}: the sidecar is byte-identical");
        d.app_mut().session_mut().unwrap().open_variant(photo).unwrap();
    }
    // A Tuning change and the wands fail the same way.
    d.app_mut().session_mut().unwrap().close_variant(Keep::Left).unwrap();
    for (cmd, p) in [
        ("photo.history.tuning", json!({ "id": photo, "session": 1, "fill": 0.5 })),
        ("photo.tuning.auto", json!({ "id": photo, "what": "lighting" })),
        ("photo.tuning.pick", json!({ "id": photo, "x": 0.5, "y": 0.5 })),
    ] {
        assert!(d.app_mut().run(cmd, &p).is_err(), "{cmd} goes to the variant on the right side, and none is open");
    }
    assert_eq!(edits(&d, photo), before);
}

#[test]
fn in_a_b_the_histogram_is_the_focused_photos() {
    let dir = tempfile::tempdir().unwrap();
    let (_, mut d, ids) = editing(dir.path());
    click(&mut d, "button:compare.two");
    assert_eq!((d.state().edit.focus, d.state().edit.second), (Side::Right, Some(ids[1])));
    let preview = d.app().session().unwrap().preview().unwrap();
    assert_eq!(preview.photo, ids[1], "the focused side's pane");
    let h = &preview.histogram;
    let want: [Vec<u64>; 3] = [&h.red, &h.green, &h.blue].map(|c| c.iter().map(|&n| u64::from(n)).collect());
    let shown = histogram::shown_bins(d.ctx()).expect("the histogram is drawn");
    assert_eq!(*shown, want, "the histogram is the right photo's");
}
