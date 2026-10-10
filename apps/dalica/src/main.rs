//! Dalica desktop app: the eframe window. M0: an empty window on the `chrome` colour.
//!
//! Usage: `dalica [--version | --help]`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod logging;

use std::process::ExitCode;

use dalica_ui_kit::Tokens;

const USAGE: &str = "usage: dalica [--version | --help]";

/// Default and minimum window sizes (`docs/ui-layout.md` §11).
const WINDOW_SIZE: [f32; 2] = [1400.0, 900.0];
const MIN_WINDOW_SIZE: [f32; 2] = [900.0, 600.0];

struct App {
    tokens: Tokens,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.tokens.chrome)).show(ui, |_ui| {});
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("dalica {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("dalica: unknown argument `{other}`\n{USAGE}");
            return ExitCode::from(2);
        }
        None => {}
    }
    logging::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Dalica").with_inner_size(WINDOW_SIZE).with_min_inner_size(MIN_WINDOW_SIZE),
        ..Default::default()
    };
    let tokens = Tokens::default();
    match eframe::run_native("Dalica", options, Box::new(move |_cc| Ok(Box::new(App { tokens })))) {
        Ok(()) => {
            log::info!("dalica: closed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            log::error!("dalica: the window failed: {e}");
            ExitCode::FAILURE
        }
    }
}
