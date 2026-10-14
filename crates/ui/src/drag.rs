//! Dragging photos (`ui-layout.md` §5.5, `control-protocol.md` §6.6): a press on a thumbnail held
//! and moved past egui's drag threshold starts a drag of the selected photos when the thumbnail is
//! selected, else of that one photo (`crate::panels::light_table::input`); while it lasts the view
//! state's [`Drag`] says where it started, what it carries, the drop target under the pointer, what
//! a drop there would do and the drag pill's text, and `ui.inspect` → `drag` reports it; releasing
//! the button ends it.
//!
//! A stub until M9.23: no target is found and a release drops nothing; M9.23 adds the targets
//! (tree folders and albums, the tray, the gaps of an album), the pill (M9.5's `drag_pill`), the
//! drop and Esc.

use dalica_core::PhotoId;
use serde_json::Value;

use crate::app::App;
use crate::state::{Drag, DragOp, MAX_IDS};

/// Starts a drag of `ids` from the widget `from` (`thumb:12`). A drag already running is replaced.
pub fn start(app: &mut App, from: String, mut ids: Vec<PhotoId>) {
    ids.truncate(MAX_IDS);
    let pill = pill(None, ids.len());
    app.state_mut().drag = Some(Drag { from, ids, target: None, op: None, pill });
}

/// The pointer moved during a drag: the target under it (a widget id) and what a drop there would
/// do. The stub finds no target.
pub fn hover(app: &mut App, target: Option<String>, op: Option<DragOp>) {
    if let Some(d) = app.state_mut().drag.as_mut() {
        d.pill = pill(op, d.ids.len());
        d.target = target;
        d.op = op;
    }
}

/// The button was released (or the drag cancelled): the drag ends. The stub drops nothing.
pub fn end(app: &mut App) {
    app.state_mut().drag = None;
}

/// Whether a drag is in progress.
pub fn dragging(app: &App) -> bool {
    app.state().drag.is_some()
}

/// The drag pill's text for `n` photos and what a drop would do: "Move 3 photos", "Copy 3 photos",
/// "Add to album"; with no target the count alone.
pub fn pill(op: Option<DragOp>, n: usize) -> String {
    let photos = if n == 1 { "1 photo".to_string() } else { format!("{n} photos") };
    match op {
        None => photos,
        Some(DragOp::Move) => format!("Move {photos}"),
        Some(DragOp::Copy) => format!("Copy {photos}"),
        Some(DragOp::Add) => "Add to album".into(),
        Some(DragOp::Reorder) => format!("Move {photos} here"),
        Some(DragOp::Hold) => format!("Hold {photos}"),
    }
}

/// `ui.inspect` → `drag`: the drag in progress, `null` when none.
pub fn inspect(app: &App) -> Value {
    app.state().drag.as_ref().and_then(|d| serde_json::to_value(d).ok()).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_drag_starts_follows_its_target_and_ends() {
        let mut app = App::new(None);
        assert_eq!(inspect(&app), Value::Null);
        start(&mut app, "thumb:3".into(), vec![PhotoId::new(3), PhotoId::new(4)]);
        assert!(dragging(&app));
        assert_eq!(inspect(&app), json!({ "from": "thumb:3", "ids": [3, 4], "target": null, "op": null, "pill": "2 photos" }));
        hover(&mut app, Some("tree:folder:2".into()), Some(DragOp::Copy));
        assert_eq!((inspect(&app)["op"].clone(), inspect(&app)["pill"].clone()), (json!("copy"), json!("Copy 2 photos")));
        end(&mut app);
        assert!(!dragging(&app));
        assert_eq!(pill(Some(DragOp::Move), 1), "Move 1 photo");
        assert_eq!(pill(Some(DragOp::Add), 5), "Add to album");
        // Nothing to follow without a drag.
        hover(&mut app, None, None);
        assert_eq!(inspect(&app), Value::Null);
        // Not part of ui.state.
        start(&mut app, "thumb:1".into(), vec![PhotoId::new(1)]);
        assert!(serde_json::to_value(app.state()).unwrap().get("drag").is_none());
    }
}
