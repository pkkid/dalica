//! Notices: what the session reports on its own (a metadata write that failed, a file changed in
//! another application) shown as toasts (`design-system.md` §6 `toast`) at the bottom of the light
//! table. The app polls the session once a frame ([`Notices::poll`]), draws the toasts still showing
//! ([`Notices::show`], `toast:<n>`) and reports them to `ui.inspect` → `toasts` ([`Notices::inspect`]).
//! Each toast stays its kind's time (1.4 s, failures 6 s), then is dropped.
//!
//! The session has no events to report yet: the metadata write-back adds its failed writes later in
//! this milestone, through [`Notices::poll`]. Until then only [`Notices::push`] adds a toast.

use dalica_engine::Session;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{self, TOAST_ERROR_SECS, TOAST_SECS, ToastKind};
use egui::{Rect, Ui};
use serde_json::{Value, json};

/// The most toasts kept at once; a new one beyond it drops the oldest.
pub const MAX_TOASTS: usize = 8;
/// The longest toast text kept, in characters.
pub const MAX_TEXT: usize = 300;

/// One toast: its number (the `<n>` of `toast:<n>`), text, kind and when it appeared (seconds on
/// the frame clock).
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub n: u64,
    pub text: String,
    pub kind: ToastKind,
    pub shown_at: f64,
}

impl Toast {
    /// How long it shows, in seconds.
    fn life(&self) -> f64 {
        match self.kind {
            ToastKind::Info => TOAST_SECS,
            ToastKind::Error => TOAST_ERROR_SECS,
        }
    }

    /// The seconds it still shows at `now` (0 once over).
    pub fn remaining(&self, now: f64) -> f64 {
        let age = now - self.shown_at;
        if age.is_finite() { (self.life() - age.max(0.0)).max(0.0) } else { 0.0 }
    }
}

/// The toasts showing now.
#[derive(Debug, Default)]
pub struct Notices {
    toasts: Vec<Toast>,
    next: u64,
}

impl Notices {
    /// Shows `text` as a toast of `kind` from `now` on.
    pub fn push(&mut self, text: &str, kind: ToastKind, now: f64) {
        let mut text: String = text.chars().take(MAX_TEXT).collect();
        text.retain(|c| !c.is_control());
        self.next = self.next.saturating_add(1);
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast { n: self.next, text, kind, shown_at: if now.is_finite() { now } else { 0.0 } });
    }

    /// Turns what `session` reported since the last poll into toasts, and drops the toasts whose
    /// time is over. Cheap when nothing happened: call it once a frame.
    pub fn poll(&mut self, _session: &Session, now: f64) {
        self.toasts.retain(|t| t.remaining(now) > 0.0);
    }

    /// The toasts still showing at `now`, oldest first.
    pub fn showing(&self, now: f64) -> impl Iterator<Item = &Toast> {
        self.toasts.iter().filter(move |t| t.remaining(now) > 0.0)
    }

    /// Draws the newest toast still showing at the bottom of `area` (the light table).
    pub fn show(&self, ui: &Ui, t: &Tokens, area: Rect, now: f64) {
        if let Some(toast) = self.showing(now).last() {
            widgets::toast(ui, t, &format!("toast:{}", toast.n), area, &toast.text, toast.kind, now - toast.shown_at, None);
        }
    }

    /// `ui.inspect` → `toasts`: `[{id, text, kind, remaining}]` (`kind` `info` or `error`,
    /// `remaining` in seconds).
    pub fn inspect(&self, now: f64) -> Value {
        let list: Vec<Value> = self
            .showing(now)
            .map(|toast| {
                let kind = match toast.kind {
                    ToastKind::Info => "info",
                    ToastKind::Error => "error",
                };
                json!({ "id": format!("toast:{}", toast.n), "text": toast.text, "kind": kind, "remaining": toast.remaining(now) })
            })
            .collect();
        Value::Array(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_show_their_time_and_are_bounded() {
        let mut n = Notices::default();
        assert_eq!(n.inspect(0.0), json!([]));
        n.push("Starred 12 photos", ToastKind::Info, 10.0);
        n.push("Could not write the caption\u{7} of IMG_1.jpg", ToastKind::Error, 10.5);
        let v = n.inspect(11.0);
        assert_eq!(v[0]["id"], "toast:1");
        assert_eq!((v[1]["id"].as_str(), v[1]["kind"].as_str()), (Some("toast:2"), Some("error")));
        assert_eq!(v[1]["text"], "Could not write the caption of IMG_1.jpg", "control characters dropped");
        assert!((v[0]["remaining"].as_f64().unwrap() - 0.4).abs() < 1e-9);
        // The info toast is over at 11.4; the failure stays 6 s.
        assert_eq!(n.showing(11.5).count(), 1);
        assert_eq!(n.showing(16.5).count(), 0);
        for i in 0..20 {
            n.push(&"x".repeat(1000), ToastKind::Info, 20.0 + f64::from(i));
        }
        assert_eq!(n.toasts.len(), MAX_TOASTS);
        assert!(n.toasts.iter().all(|t| t.text.chars().count() == MAX_TEXT));
        assert_eq!(n.toasts.last().map(|t| t.n), Some(22));
        // Hostile clocks never show a toast forever.
        let mut h = Notices::default();
        h.push("a", ToastKind::Info, f64::NAN);
        assert_eq!(h.showing(f64::INFINITY).count(), 0);
        assert_eq!(h.showing(f64::NAN).count(), 0);
    }
}
