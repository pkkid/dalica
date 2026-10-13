//! Dates: `photo.dates.set` (Adjust Date and Time; `organizing.md` §9). Tools ▸ Adjust Date and
//! Time… (the UI's `photo.adjustDate`) opens the dialog; its OK runs this command once.
//!
//! - `{ids, shift}`: every photo moves by the same number of seconds from its effective date (the
//!   date the user set, else the file's capture date, else its modification time).
//! - `{ids, set}`: every photo gets the same date and time (`"YYYY-MM-DDTHH:MM:SS"`).
//!
//! The new date is the user's date of the photo ([`dalica_library::PhotoInfo::date`]; the file's
//! own date stays beside it); a new date equal to the file's own capture date clears the user's
//! date instead, so shifting there and back leaves nothing behind. All photos change in one
//! library undo step (none when no date changes), and the folder's photos re-sort by the new
//! dates ([`dalica_library::Library::photos_in`]).
//!
//! The undo step sets each photo's date back to the effective date it had, written out as a date
//! of the user's (not "the file's date"): once the new date is written into the file and a rescan
//! reads it back as the file's own, falling back to the file's date would keep the new one. So
//! undo and redo change the effective date whether or not the write had landed, and the metadata
//! write-back puts the file's date back (`metadata.rs`). Undo and redo apply a step's dates through
//! [`without_file_dates`]: a date equal to the photo's own file date goes back to "the file's
//! date", so when nothing was read back in between, undo restores the library exactly. When the
//! file's date did change, the photo keeps a date of the user's (in its sidecar) that its file
//! will hold once the write lands.

use dalica_core::{Date, DateTime, PhotoId};
use dalica_library::Op;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::photo::{has_ids, known};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "photo.dates.set",
        label: "Adjust Date",
        menu: "",
        keys: &[],
        params: r#"{"ids":[PhotoId], "shift":int} | {"ids":[PhotoId], "set":"YYYY-MM-DDTHH:MM:SS"}: shift every photo's date taken by the same number of seconds (from its effective date, else the file time), or set them all to one date and time; replies {ids, changed, dates: [{id, date}]}"#,
        effect: Effect::Library,
        history: History::Library,
        enabled: has_ids,
        run: set,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetDates {
    ids: Vec<PhotoId>,
    #[serde(default)]
    shift: Option<i64>,
    #[serde(default)]
    set: Option<String>,
}

/// How the dates change.
#[derive(Clone, Copy)]
enum How {
    Shift(i64),
    Set(DateTime),
}

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SetDates { ids, shift, set } = params(p)?;
    let how = match (shift, set) {
        (Some(secs), None) => How::Shift(secs),
        (None, Some(text)) => How::Set(text.parse().map_err(|e| CmdError::BadParams(format!("set: {e}")))?),
        _ => return Err(CmdError::BadParams("give either shift (seconds) or set (a date and time)".into())),
    };
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let lib = s.library();
    let mut changed: Vec<(PhotoId, Option<DateTime>)> = Vec::new();
    // The undo: each changed photo's effective date before, written out.
    let mut before: Vec<(PhotoId, Option<DateTime>)> = Vec::new();
    let mut dates = Vec::with_capacity(ids.len());
    for id in &ids {
        let photo = lib.photo(*id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        let new = match how {
            How::Set(date) => date,
            How::Shift(secs) => {
                let from = lib.sort_date(photo).ok_or_else(|| CmdError::Limit(format!("photo {id} has no date to shift")))?;
                shifted(from, secs).ok_or_else(|| CmdError::Limit(format!("shifting photo {id} by {secs} s leaves the years 1 to 9999")))?
            }
        };
        // Back to the file's own date: the user's date goes.
        let stored = Some(new).filter(|d| photo.date != Some(*d));
        let info = lib.info(*id);
        if info.date != stored {
            changed.push((*id, stored));
            before.push((*id, info.date.or(photo.date)));
        }
        dates.push(json!({ "id": id, "date": new }));
    }
    let n = changed.len();
    if n > 0 {
        s.apply(Op::SetDates { dates: changed })?;
        s.set_undo_op(Op::SetDates { dates: before });
    }
    Ok(json!({ "ids": ids, "changed": n, "dates": dates }))
}

/// The most nested batches [`without_file_dates`] walks into; deeper ones are left as they are.
const MAX_DEPTH: usize = 64;

/// `op` with every date of `Op::SetDates` equal to its photo's own file date in `library` turned
/// into `None` (the file's date), so a step never leaves a user's date that says what the file
/// says. Undo and redo apply their steps through this (`Session::apply_step`).
pub(crate) fn without_file_dates(library: &dalica_library::Library, op: Op) -> Op {
    normalise(library, op, 0)
}

fn normalise(library: &dalica_library::Library, op: Op, depth: usize) -> Op {
    match op {
        Op::SetDates { dates } => Op::SetDates {
            dates: dates.into_iter().map(|(id, d)| (id, d.filter(|d| library.photo(id).is_none_or(|p| p.date != Some(*d))))).collect(),
        },
        Op::Batch { ops } if depth < MAX_DEPTH => Op::Batch { ops: ops.into_iter().map(|o| normalise(library, o, depth + 1)).collect() },
        other => other,
    }
}

/// `date` moved by `secs` seconds, or `None` outside the years 1 to 9999.
pub(crate) fn shifted(date: DateTime, secs: i64) -> Option<DateTime> {
    DateTime::from_unix_secs(unix_secs(date).checked_add(secs)?)
}

/// Seconds since 1970-01-01T00:00:00 of `date`, read as UTC (the inverse of
/// [`DateTime::from_unix_secs`]).
pub(crate) fn unix_secs(date: DateTime) -> i64 {
    let days = days_from_civil(date.date());
    days * 86_400 + i64::from(date.hour()) * 3600 + i64::from(date.minute()) * 60 + i64::from(date.second())
}

/// Days since 1970-01-01 of `date` (Howard Hinnant's `days_from_civil`, the inverse of the
/// `civil_from_days` in `dalica-core`). Years 1..=9999 keep every value far inside `i64`.
fn days_from_civil(date: Date) -> i64 {
    let (m, d) = (i64::from(date.month()), i64::from(date.day()));
    let y = i64::from(date.year()) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::cmd::testing::{hostile, library};

    fn dt(s: &str) -> DateTime {
        s.parse().unwrap()
    }

    #[test]
    fn seconds_round_trip_over_the_whole_calendar() {
        for s in
            ["0001-01-01T00:00:00", "1969-12-31T23:59:59", "1970-01-01T00:00:00", "2000-02-29T12:00:00", "2026-10-12T08:30:15", "9999-12-31T23:59:59"]
        {
            let d = dt(s);
            assert_eq!(DateTime::from_unix_secs(unix_secs(d)), Some(d), "{s}");
        }
        assert_eq!(unix_secs(dt("1970-01-02T00:00:01")), 86_401);
        // Every day of a span with a leap day, one after the other.
        let mut secs = unix_secs(dt("1999-12-30T00:00:00"));
        for _ in 0..800 {
            let d = DateTime::from_unix_secs(secs).unwrap();
            assert_eq!(unix_secs(d), secs);
            secs += 86_400;
        }
        assert_eq!(shifted(dt("2026-01-31T23:30:00"), 3600), Some(dt("2026-02-01T00:30:00")));
        assert_eq!(shifted(dt("2024-02-28T23:00:00"), 3600 * 2), Some(dt("2024-02-29T01:00:00")));
        assert_eq!(shifted(dt("9999-12-31T23:59:59"), 1), None);
        assert_eq!(shifted(dt("0001-01-01T00:00:00"), -1), None);
        assert_eq!(shifted(dt("2026-01-01T00:00:00"), i64::MAX), None);
        assert_eq!(shifted(dt("2026-01-01T00:00:00"), i64::MIN), None);
    }

    #[test]
    fn disabled_and_hostile_params() {
        let (_dirs, mut s, ids) = library(1);
        assert_eq!(s.execute("photo.dates.set", &json!({ "ids": [], "shift": 60 })), Err(CmdError::Disabled("Select some photos first.".into())));
        assert!(matches!(s.execute("photo.dates.set", &json!({ "ids": [999], "shift": 60 })), Err(CmdError::NotFound(_))));
        for bad in [
            json!({ "ids": [ids[0]] }),
            json!({ "ids": [ids[0]], "shift": 1, "set": "2026-01-01T00:00:00" }),
            json!({ "ids": [ids[0]], "shift": 1.5 }),
            json!({ "ids": [ids[0]], "shift": "1" }),
            json!({ "ids": [ids[0]], "shift": 18_446_744_073_709_551_615u64 }),
            json!({ "ids": [ids[0]], "set": "2026-02-30T00:00:00" }),
            json!({ "ids": [ids[0]], "set": "yesterday" }),
            json!({ "ids": [ids[0]], "set": 5 }),
            json!({ "ids": [ids[0]], "set": null }),
        ] {
            assert!(matches!(s.execute("photo.dates.set", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(s.execute("photo.dates.set", &json!({ "ids": [ids[0]], "shift": i64::MAX })), Err(CmdError::Limit(_))));
        hostile(&mut s, "photo.dates.set");
        assert_eq!(s.undo_stack().len(), 0);
    }
}
