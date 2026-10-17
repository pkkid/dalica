//! Moving the window by its title bar: every press and drag on the bar's empty part starts a move,
//! including the one after a move whose button release the window manager kept (it does, on
//! Wayland and X11, while it moves the window). A click on a caption button minimizes, maximizes or
//! closes the window, and the bar does the same over the "Opening the library…" and "could not be
//! opened" screens, before any library is open.

use std::sync::Arc;

use crate::common::new_driver;
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::{App, Driver};
use egui::{PointerButton, ViewportCommand};

fn driver() -> (dalica_fs::TempDir, Driver) {
    let dir = dalica_engine::demo::fresh_dir().unwrap();
    let session = crate::common::demo_open(dir.path(), Arc::new(dalica_core::SystemClock));
    let d = new_driver(App::new(Some(session)), 1400, 900, 1.0);
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

/// The window commands of `cmds` that the caption buttons and the bar send.
fn window_cmds(cmds: &[ViewportCommand]) -> Vec<ViewportCommand> {
    cmds.iter()
        .filter(|c| matches!(c, ViewportCommand::Minimized(_) | ViewportCommand::Maximized(_) | ViewportCommand::Close | ViewportCommand::StartDrag))
        .cloned()
        .collect()
}

#[test]
fn a_click_on_a_caption_button_minimizes_maximizes_or_closes() {
    let (_dir, mut d) = driver();
    d.take_window_commands();
    for (id, want) in [
        ("button:window.minimize", ViewportCommand::Minimized(true)),
        ("button:window.maximize", ViewportCommand::Maximized(true)),
        ("button:window.close", ViewportCommand::Close),
    ] {
        d.click(id).unwrap();
        assert_eq!(window_cmds(&d.take_window_commands()), [want], "{id}");
    }
}

/// The window before a library is open: the opening screen (the app without a session) or the
/// "could not be opened" screen, with the bare title bar over it, as the desktop window draws them
/// (`apps/dalica/src/window.rs`, `bare_title_bar`). Frames run on egui directly: the driver draws
/// the app's frame alone.
struct Bare {
    ctx: egui::Context,
    app: App,
    failed: bool,
    time: f64,
}

impl Bare {
    fn new(failed: bool) -> Bare {
        let ctx = egui::Context::default();
        let app = App::new(None);
        app.install(&ctx);
        let mut b = Bare { ctx, app, failed, time: 0.0 };
        for _ in 0..3 {
            b.frame(Vec::new());
        }
        b
    }

    /// One frame with `events`; the window commands it sent.
    fn frame(&mut self, events: Vec<egui::Event>) -> Vec<ViewportCommand> {
        let mut input = dalica_ui_kit::raw_input(1400, 900, 1.0);
        input.time = Some(self.time);
        input.events = events;
        let (app, failed) = (&mut self.app, self.failed);
        let mut out = self.ctx.run_ui(input, |ui| {
            if failed {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.label("The library could not be opened");
                });
            } else {
                app.frame(ui);
            }
            let content = ui.ctx().content_rect();
            let bar = egui::Rect::from_min_size(content.min, egui::vec2(content.width(), dalica_ui::panels::layout::MENU_BAR));
            let mut row = ui.new_child(egui::UiBuilder::new().id_salt("dalica-bare-title-bar").max_rect(bar.shrink2(egui::vec2(8.0, 0.0))));
            row.set_clip_rect(bar);
            dalica_ui::titlebar::show(&mut row, app, bar, None);
        });
        out.textures_delta.clear();
        self.time += 1.0 / 60.0;
        out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    }

    fn centre(&self, id: &str) -> egui::Pos2 {
        let w = dalica_ui_kit::registry::find(&self.ctx, id).unwrap_or_else(|| panic!("no {id}"));
        egui::pos2(w.rect[0] + w.rect[2] / 2.0, w.rect[1] + w.rect[3] / 2.0)
    }

    /// Plays `events` one frame each, then one more; the window commands they sent.
    fn play(&mut self, events: Vec<egui::Event>) -> Vec<ViewportCommand> {
        let mut cmds = Vec::new();
        for e in events {
            cmds.extend(self.frame(vec![e]));
        }
        cmds.extend(self.frame(Vec::new()));
        window_cmds(&cmds)
    }
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

#[test]
fn the_title_bar_works_over_the_opening_and_could_not_open_screens() {
    for failed in [false, true] {
        let screen = if failed { "could not open" } else { "opening" };
        let mut b = Bare::new(failed);
        assert!(b.app.session().is_none());
        for (id, want) in [
            ("button:window.minimize", ViewportCommand::Minimized(true)),
            ("button:window.maximize", ViewportCommand::Maximized(true)),
            ("button:window.close", ViewportCommand::Close),
        ] {
            let at = b.centre(id);
            let got = b.play(vec![egui::Event::PointerMoved(at), button(at, true), button(at, false)]);
            assert_eq!(got, [want], "{screen}: {id}");
        }
        // A press and drag on the bar moves the window; a double-click maximizes.
        let at = b.centre("titlebar:drag");
        let got = b.play(vec![
            egui::Event::PointerMoved(at),
            button(at, true),
            egui::Event::PointerMoved(at + egui::vec2(15.0, 0.0)),
            egui::Event::PointerMoved(at + egui::vec2(30.0, 0.0)),
            button(at + egui::vec2(30.0, 0.0), false),
        ]);
        assert_eq!(got, [ViewportCommand::StartDrag], "{screen}: drag");
        b.time += 1.0;
        let got = b.play(vec![egui::Event::PointerMoved(at), button(at, true), button(at, false), button(at, true), button(at, false)]);
        assert_eq!(got, [ViewportCommand::Maximized(true)], "{screen}: double-click");
    }
}
