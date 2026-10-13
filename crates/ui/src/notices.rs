//! Notices: what the session reports on its own (a metadata write that failed, a file changed in
//! another application) shown as toasts (`design-system.md` §6 `toast`) at the bottom of the light
//! table. The app polls the session once a frame ([`Notices::poll`]), draws the toasts still showing
//! ([`Notices::show`], `toast:<n>`) and reports them to `ui.inspect` → `toasts` ([`Notices::inspect`]).
//! Each toast stays its kind's time (1.4 s, cautions and failures 6 s), then is dropped. Its text
//! wraps to at most three lines; only a longer one is cut short, in the middle, and never inside
//! the file name it is about.
//!
//! Today the session reports failed metadata writes (`dalica_engine::metadata`): each becomes one
//! `caution` toast naming the file and the fields (nothing is lost: the folder sidecar keeps the
//! values; `data-model.md` §5, `xmp-interop.md` §1). The app's own confirmations come through
//! [`Notices::push`].

use dalica_engine::Session;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{self, ToastKind, ToastText};
use egui::{Rect, Ui};
use serde_json::{Value, json};

/// The most toasts kept at once; a new one beyond it drops the oldest.
pub const MAX_TOASTS: usize = 8;
/// The longest toast text kept, in characters.
pub const MAX_TEXT: usize = 300;

/// One toast: its number (the `<n>` of `toast:<n>`), text, how many characters at the start of
/// the text stay whole if it is cut short, kind and when it appeared (seconds on the frame clock).
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub n: u64,
    pub text: String,
    pub keep: usize,
    pub kind: ToastKind,
    pub shown_at: f64,
}

impl Toast {
    /// How long it shows, in seconds.
    fn life(&self) -> f64 {
        self.kind.life()
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
    /// The number of the last session event shown ([`dalica_engine::MetadataEvent::seq`]).
    seen: u64,
}

impl Notices {
    /// Shows `text` as a toast of `kind` from `now` on.
    pub fn push(&mut self, text: &str, kind: ToastKind, now: f64) {
        self.push_keeping(text, "", kind, now);
    }

    /// Shows `text` as a toast of `kind` from `now` on; if it has to be cut short, the text up to
    /// and including `whole` (a file name in it; its occurrence followed by ": " first) stays
    /// whole.
    pub fn push_keeping(&mut self, text: &str, whole: &str, kind: ToastKind, now: f64) {
        let text = clean(text);
        let whole = clean(whole);
        let keep = if whole.is_empty() {
            0
        } else {
            let at = text.find(&format!("{whole}: ")).or_else(|| text.find(&whole));
            at.map_or(0, |i| text.get(..i).map_or(0, |s| s.chars().count()).saturating_add(whole.chars().count()))
        };
        self.next = self.next.saturating_add(1);
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast { n: self.next, text, keep, kind, shown_at: if now.is_finite() { now } else { 0.0 } });
    }

    /// Turns what `session` reported since the last poll into toasts, and drops the toasts whose
    /// time is over. Cheap when nothing happened: call it once a frame.
    pub fn poll(&mut self, session: &Session, now: f64) {
        self.toasts.retain(|t| t.remaining(now) > 0.0);
        let fresh: Vec<(u64, String, String)> = session
            .metadata_events()
            .filter(|e| e.seq > self.seen)
            .map(|e| (e.seq, e.message(), e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
            .collect();
        for (seq, text, file) in fresh {
            self.seen = self.seen.max(seq);
            self.push_keeping(&text, &file, ToastKind::Caution, now);
        }
    }

    /// The toasts still showing at `now`, oldest first.
    pub fn showing(&self, now: f64) -> impl Iterator<Item = &Toast> {
        self.toasts.iter().filter(move |t| t.remaining(now) > 0.0)
    }

    /// Draws the newest toast still showing at the bottom of `area` (the light table).
    pub fn show(&self, ui: &Ui, t: &Tokens, area: Rect, now: f64) {
        if let Some(toast) = self.showing(now).last() {
            let text = ToastText::new(&toast.text, toast.keep);
            widgets::toast(ui, t, &format!("toast:{}", toast.n), area, text, toast.kind, now - toast.shown_at, None);
        }
    }

    /// `ui.inspect` → `toasts`: `[{id, text, kind, remaining}]` (`kind` `info`, `caution` or `error`,
    /// `remaining` in seconds).
    pub fn inspect(&self, now: f64) -> Value {
        let list: Vec<Value> = self
            .showing(now)
            .map(|toast| {
                let kind = match toast.kind {
                    ToastKind::Info => "info",
                    ToastKind::Caution => "caution",
                    ToastKind::Error => "error",
                };
                json!({ "id": format!("toast:{}", toast.n), "text": toast.text, "kind": kind, "remaining": toast.remaining(now) })
            })
            .collect();
        Value::Array(list)
    }
}

/// `text` at most [`MAX_TEXT`] characters long, without control characters.
fn clean(text: &str) -> String {
    text.chars().take(MAX_TEXT).filter(|c| !c.is_control()).collect()
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

    #[test]
    fn the_file_name_is_what_stays_whole() {
        let mut n = Notices::default();
        n.push_keeping("Could not write the caption into a.jpg: the file is read-only", "a.jpg", ToastKind::Caution, 0.0);
        n.push_keeping("Could not write the caption into caption: no", "caption", ToastKind::Caution, 0.0);
        n.push_keeping("Could not write tags into b\u{7}.jpg", "b\u{7}.jpg", ToastKind::Caution, 0.0);
        n.push_keeping("No file here", "c.jpg", ToastKind::Caution, 0.0);
        n.push("Starred 1 photo", ToastKind::Info, 0.0);
        let keeps: Vec<usize> = n.toasts.iter().map(|t| t.keep).collect();
        assert_eq!(keeps, [38, 40, 31, 0, 0], "the name followed by \": \" first; control characters dropped from both");
        assert_eq!(n.inspect(1.0)[0]["kind"], "caution");
    }
}
