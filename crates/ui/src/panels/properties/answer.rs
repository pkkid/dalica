//! One photo's rows in the Properties pane: the `photo.metadata {id, all?}` reply (`ui-layout.md`
//! §7) read into an [`Answer`], and when to ask for it.
//!
//! - **Reading the reply** ([`read`]): the rows in the engine's order, dates written out in full
//!   ("June 4, 2026 7:42:10 PM"), the Dimensions row noting the file's own size when the photo's
//!   recipe turns it, the "Changed in another application; Dalica's value kept" note on the rows it
//!   is about (or on the pane when no row shows that field), the read error, and with `all` the
//!   Show All Metadata fields grouped as the file holds them (EXIF, GPS, IPTC, XMP …). Values are
//!   made one line (control characters become spaces) and capped at [`MAX_VALUE`] characters.
//! - **Asking** ([`answer`]): the reply is kept per pane and asked again only when the photo, the
//!   Show All switch or the library's revision changed, while it is `pending` (the file is being
//!   read on the engine's workers; a repaint is requested every [`POLL`] so the rows arrive without
//!   input), and once every [`REASK`] seconds otherwise (a note or a retried read changes no
//!   revision). Asking is a cache lookup: no frame reads the file.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{DateTime, PhotoId};
use egui::Id;
use serde_json::{Value, json};

use crate::app::App;
use crate::panels::light_table::long_date;

/// How often a frame is asked for while the file is being read.
pub const POLL: Duration = Duration::from_millis(50);
/// The longest a ready answer is kept before it is asked for again (seconds).
pub const REASK: f64 = 1.0;
/// The most characters of one value kept for display.
pub const MAX_VALUE: usize = 2_000;
/// The engine's note for a field another application changed (`xmp-interop.md` §5 rule 3).
pub const CHANGED_ELSEWHERE: &str = "Changed in another application; Dalica's value kept";

/// One label/value row. `key` is the engine's (`dateTaken`), or the group for a Show All field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub label: String,
    pub value: String,
    /// A dim line under the value ("date from file", the changed-elsewhere note).
    pub note: Option<String>,
}

/// One group of Show All Metadata ("EXIF", "XMP", "IPTC" …) with its fields in the file's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub rows: Vec<Row>,
}

/// What the pane shows of one photo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// The file is still being read: only the library's rows are here yet.
    pub pending: bool,
    pub rows: Vec<Row>,
    /// The changed-elsewhere note when no row shows the field it is about.
    pub note: Option<String>,
    /// Why the file could not be read (its rows from the library still shown).
    pub error: Option<String>,
    /// Show All Metadata: `Some` once the file was read with `all`.
    pub groups: Option<Vec<Group>>,
    /// The engine listed only the first fields.
    pub truncated: bool,
}

/// `June 4, 2026 7:42:10 PM`: a date and time in full, with its seconds.
pub fn date_time(d: DateTime) -> String {
    let (hour, half) = match d.hour() {
        0 => (12, "AM"),
        h @ 1..=11 => (h, "AM"),
        12 => (12, "PM"),
        h => (h.saturating_sub(12), "PM"),
    };
    format!("{} {hour}:{:02}:{:02} {half}", long_date(d.date()), d.minute(), d.second())
}

/// `text` on one line (control characters as spaces), at most [`MAX_VALUE`] characters ("…" at
/// the end when cut).
pub fn one_line(text: &str) -> String {
    let mut out: String = text.chars().take(MAX_VALUE).map(|c| if c.is_control() { ' ' } else { c }).collect();
    if text.chars().nth(MAX_VALUE).is_some() {
        out.push('…');
    }
    out
}

/// The rows a changed field is shown in.
fn rows_of_field(field: &str) -> &'static [&'static str] {
    match field {
        "caption" => &["caption"],
        "tags" => &["tags"],
        "place" => &["gps"],
        "date" => &["dateTaken"],
        "star" => &["rating"],
        _ => &[],
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(one_line)
}

/// Reads a `photo.metadata` reply. `file` is the photo's size as its file shows it (the library's
/// width and height): when the Dimensions row (the size after the recipe) differs, the row notes
/// it. Entries that are not as documented are left out.
pub fn read(reply: &Value, file: Option<(u32, u32)>) -> Answer {
    let mut rows: Vec<Row> = reply
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let key = r.get("key")?.as_str()?.to_owned();
                    let raw = r.get("value")?.as_str()?;
                    let value = match key.as_str() {
                        "dateTaken" | "dateDigitized" | "fileModified" => DateTime::from_str(raw).map_or_else(|_| one_line(raw), date_time),
                        _ => one_line(raw),
                    };
                    Some(Row { key, label: text(r, "label")?, value, note: text(r, "note") })
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some((w, h)) = file.filter(|(w, h)| *w > 0 && *h > 0)
        && let Some(dims) = rows.iter_mut().find(|r| r.key == "dimensions")
    {
        let stored = format!("{w} × {h}");
        if dims.value != stored {
            dims.note = Some(format!("{stored} in the file"));
        }
    }
    let mut note = None;
    if let Some(changed) = reply.get("changedElsewhere") {
        let said = text(changed, "note").unwrap_or_else(|| CHANGED_ELSEWHERE.to_owned());
        let fields: Vec<&str> =
            changed.get("fields").and_then(Value::as_array).map(|f| f.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let mut on_a_row = false;
        for field in &fields {
            for row in rows.iter_mut().filter(|r| rows_of_field(field).contains(&r.key.as_str())) {
                row.note = Some(said.clone());
                on_a_row = true;
            }
        }
        if !on_a_row {
            note = Some(said);
        }
    }
    let groups = reply.get("all").map(|all| {
        let mut groups: Vec<Group> = Vec::new();
        for r in all.get("rows").and_then(Value::as_array).into_iter().flatten() {
            let (Some(group), Some(label), Some(value)) = (text(r, "group"), text(r, "label"), text(r, "value")) else { continue };
            let row = Row { key: group.clone(), label, value, note: None };
            match groups.iter_mut().find(|g| g.name == group) {
                Some(g) => g.rows.push(row),
                None => groups.push(Group { name: group, rows: vec![row] }),
            }
        }
        groups
    });
    Answer {
        pending: reply.get("pending").and_then(Value::as_bool).unwrap_or(false),
        rows,
        note,
        error: text(reply, "error"),
        truncated: reply.get("all").and_then(|a| a.get("truncated")).and_then(Value::as_bool).unwrap_or(false),
        groups,
    }
}

/// The last reply and what it was asked for.
#[derive(Clone, Default)]
struct Asked {
    id: Option<PhotoId>,
    all: bool,
    revision: u64,
    at: f64,
    answer: Arc<Answer>,
}

/// Photo `id`'s answer, with Show All's fields when `all`: the kept one, or asked of the engine
/// (see the module's *Asking*). While the file is being read a repaint is requested every
/// [`POLL`], so the rows arrive without input.
pub fn answer(app: &mut App, ctx: &egui::Context, id: PhotoId, all: bool) -> Arc<Answer> {
    let key = Id::new("properties.answer");
    let now = ctx.input(|i| i.time);
    let Some(session) = app.session_mut() else { return Arc::default() };
    let revision = session.library().revision();
    let kept = ctx.data(|d| d.get_temp::<Asked>(key));
    if let Some(k) = &kept
        && k.id == Some(id)
        && k.all == all
        && k.revision == revision
        && !k.answer.pending
        && (k.at..k.at + REASK).contains(&now)
    {
        return Arc::clone(&k.answer);
    }
    let file = session.library().photo(id).map(|p| (p.width, p.height));
    let answer = match session.execute("photo.metadata", &json!({ "id": id, "all": all })) {
        Ok(reply) => read(&reply, file),
        Err(e) => Answer { error: Some(one_line(&e.to_string())), ..Answer::default() },
    };
    if answer.pending {
        ctx.request_repaint_after(POLL);
    }
    let answer = Arc::new(answer);
    // An unchanged answer keeps its allocation, so what was drawn from it stays shared.
    let answer = match kept {
        Some(k) if k.answer == answer => k.answer,
        _ => answer,
    };
    ctx.data_mut(|d| d.insert_temp(key, Asked { id: Some(id), all, revision, at: now, answer: Arc::clone(&answer) }));
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, label: &str, value: &str) -> Value {
        json!({ "key": key, "label": label, "value": value })
    }

    #[test]
    fn dates_are_written_in_full_and_values_on_one_line() {
        let d = |s: &str| date_time(DateTime::from_str(s).unwrap());
        assert_eq!(d("2026-06-04T19:42:10"), "June 4, 2026 7:42:10 PM");
        assert_eq!(d("2026-06-04T00:05:09"), "June 4, 2026 12:05:09 AM");
        assert_eq!(d("2026-06-04T12:00:00"), "June 4, 2026 12:00:00 PM");
        assert_eq!(one_line("a\nb\tc"), "a b c");
        let long = "x".repeat(MAX_VALUE + 5);
        assert_eq!(one_line(&long).chars().count(), MAX_VALUE + 1);
        assert!(one_line(&long).ends_with('…'));
        assert_eq!(one_line(&"x".repeat(MAX_VALUE)), "x".repeat(MAX_VALUE));
    }

    #[test]
    fn a_reply_reads_into_rows_notes_and_groups() {
        let reply = json!({
            "pending": false,
            "rows": [
                row("dimensions", "Dimensions", "80 × 120"),
                { "key": "dateTaken", "label": "Date taken", "value": "2026-06-04T19:42:10", "note": "date from file" },
                row("caption", "Caption", "Tram\n28"),
                row("fileModified", "File modified", "not a date"),
                { "key": "broken" },
            ],
            "changedElsewhere": { "fields": ["caption", "star"], "note": CHANGED_ELSEWHERE },
            "all": { "rows": [
                { "group": "EXIF", "label": "Make", "value": "Canon" },
                { "group": "XMP", "label": "Rating", "value": "3" },
                { "group": "EXIF", "label": "Model", "value": "EOS" },
                { "group": 1 },
            ], "truncated": true },
        });
        let a = read(&reply, Some((120, 80)));
        assert!(!a.pending && a.truncated && a.error.is_none());
        let shown: Vec<(&str, &str, Option<&str>)> = a.rows.iter().map(|r| (r.key.as_str(), r.value.as_str(), r.note.as_deref())).collect();
        assert_eq!(
            shown,
            [
                ("dimensions", "80 × 120", Some("120 × 80 in the file")),
                ("dateTaken", "June 4, 2026 7:42:10 PM", Some("date from file")),
                ("caption", "Tram 28", Some(CHANGED_ELSEWHERE)),
                ("fileModified", "not a date", None),
            ]
        );
        assert_eq!(a.note, None, "the note sits on the caption row (star has no row here)");
        let groups = a.groups.unwrap();
        assert_eq!(groups.iter().map(|g| (g.name.as_str(), g.rows.len())).collect::<Vec<_>>(), [("EXIF", 2), ("XMP", 1)]);
        assert_eq!(groups[0].rows[1].value, "EOS");

        // A note about a field no row shows goes on the pane; the same size notes nothing.
        let only = json!({ "rows": [row("dimensions", "Dimensions", "120 × 80")], "changedElsewhere": { "fields": ["place"] } });
        let a = read(&only, Some((120, 80)));
        assert_eq!(a.note.as_deref(), Some(CHANGED_ELSEWHERE));
        assert_eq!(a.rows[0].note, None);
        assert_eq!(a.groups, None, "asked without all");
    }

    #[test]
    fn hostile_replies_read_as_nothing() {
        for bad in [json!(null), json!([]), json!("rows"), json!({ "rows": 3, "all": [], "pending": "yes", "error": 1 })] {
            let a = read(&bad, None);
            assert!(a.rows.is_empty() && !a.pending && a.error.is_none(), "{bad}");
        }
        let err = read(&json!({ "pending": true, "error": "the file is missing" }), Some((0, 0)));
        assert!(err.pending);
        assert_eq!(err.error.as_deref(), Some("the file is missing"));
    }
}
