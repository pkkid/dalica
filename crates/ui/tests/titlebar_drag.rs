//! Moving the window by its title bar: every press and drag on the bar's empty part starts a move,
//! including the one after a move whose button release the window manager kept (it does, on
//! Wayland and X11, while it moves the window).

use std::sync::Arc;

use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::{App, Driver};
use egui::{PointerButton, ViewportCommand};

fn driver() -> (dalica_fs::TempDir, Driver) {
    let dir = dalica_engine::demo::fresh_dir().unwrap();
    let session = dalica_engine::demo::open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap();
    let d = Driver::new(App::new(Some(session)), 1400, 900, 1.0);
    (dir, d)
}

fn starts(cmds: &[ViewportCommand]) -> usize {
    cmds.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count()
}

/// Press on the bar and drag 30 points, without a release (the window manager takes it).
fn press_and_drag(d: &mut Driver) {
    let w = d.widgets().into_iter().find(|w| w.id == "titlebar:drag").expect("the title bar's drag area");
    let at = egui::pos2(w.rect[0] + w.rect[2] / 2.0, w.rect[1] + w.rect[3] / 2.0);
    let events = [
        PointerEvent { kind: PointerKind::Move, pos: at },
        PointerEvent { kind: PointerKind::Down, pos: at },
        PointerEvent { kind: PointerKind::Drag, pos: at + egui::vec2(15.0, 0.0) },
        PointerEvent { kind: PointerKind::Drag, pos: at + egui::vec2(30.0, 0.0) },
    ];
    d.pointer(&events, PointerButton::Primary, egui::Modifiers::NONE).unwrap();
}

#[test]
fn every_press_and_drag_on_the_title_bar_moves_the_window() {
    let (_dir, mut d) = driver();
    d.take_window_commands();
    for n in 1..=3 {
        press_and_drag(&mut d);
        assert_eq!(starts(&d.take_window_commands()), 1, "drag {n} starts a window move");
    }
}

#[test]
fn a_drag_with_its_release_still_moves_the_window_once() {
    let (_dir, mut d) = driver();
    d.take_window_commands();
    press_and_drag(&mut d);
    let at = d.pointer_pos().unwrap();
    d.pointer(&[PointerEvent { kind: PointerKind::Up, pos: at }], PointerButton::Primary, egui::Modifiers::NONE).unwrap();
    assert_eq!(starts(&d.take_window_commands()), 1);
    press_and_drag(&mut d);
    assert_eq!(starts(&d.take_window_commands()), 1);
}

#[test]
fn a_double_click_on_the_title_bar_maximizes_and_starts_no_move() {
    let (_dir, mut d) = driver();
    d.take_window_commands();
    let w = d.widgets().into_iter().find(|w| w.id == "titlebar:drag").expect("the title bar's drag area");
    let at = egui::pos2(w.rect[0] + w.rect[2] / 2.0, w.rect[1] + w.rect[3] / 2.0);
    let click = |kind| PointerEvent { kind, pos: at };
    d.pointer(
        &[click(PointerKind::Move), click(PointerKind::Down), click(PointerKind::Up), click(PointerKind::Down), click(PointerKind::Up)],
        PointerButton::Primary,
        egui::Modifiers::NONE,
    )
    .unwrap();
    let cmds = d.take_window_commands();
    assert_eq!(starts(&cmds), 0, "{cmds:?}");
    assert!(cmds.iter().any(|c| matches!(c, ViewportCommand::Maximized(true))), "{cmds:?}");
}
