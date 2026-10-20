//! Videos in the session (`editing.md` §11, `viewing.md` §4; `roadmap.md` M15 → *Video*): what
//! the session says about a video and the rules every still-only path asks, and the edit view's
//! video state.
//!
//! - **What a photo is** ([`is_video`]): a video when the library says so ([`Library::kind`]: its
//!   container's facts, else the scan's word) or its name has a video's extension (so a path
//!   never decodes a video the scan has not described yet). [`kind_fields`] gives `photo.inspect`,
//!   `library.query` and `viewer.list` its `kind` (`"still"` | `"video"`) and `durationMs`.
//! - **`video.inspect {id}`** ([`inspect`], `cmd/video.rs`): the container's facts, whether Dalica
//!   can play it ([`playable`]) and its trim ([`trim_of`]).
//! - **Whether Dalica plays it** ([`playable`]): decision-neutral until the frames are decoded
//!   (M15.15 answers by codec): every video answers [`CANNOT_PLAY`], the edit view's and the
//!   viewer's "This video can't be played".
//! - **The still-only paths** ask [`edit_refusal`] and [`CROP_REFUSED`]: a video takes only its
//!   own edits (a rotation and the trim, `editing.md` §11), never an effect, a tuning, a crop or a
//!   pasted recipe; Save refuses it by its extension (`cmd/save.rs`), Export and the slideshow
//!   leave it out and count it, a collage leaves it out with [`collage_notice`], the face scan
//!   skips it and its thumbnail is the video tile ([`crate::thumbs`]: never decoded, never the
//!   error badge).
//!
//! The edit view's state ([`Video`]): at most one video open, whether it plays, where it is; a
//! stub until M15.25 (playback, driven by the [`crate::sound`] clock) fills it; the volume is
//! [`crate::settings::VideoSettings`].

use std::path::Path;

use dalica_core::PhotoId;
use dalica_edit::Op as EditOp;
use dalica_library::{Library, PhotoKind, VideoFacts};
use serde_json::{Value, json};

use crate::session::Session;

/// What the edit view and the viewer say of a video Dalica cannot play (`viewing.md` §4).
pub const CANNOT_PLAY: &str = "This video can't be played";
/// Why Batch Edit, an effect, a tuning or Paste All Edits refuses a video.
pub const EDITS_REFUSED: &str = "A video takes no photo edits.";
/// Why Crop and Straighten refuse a video.
pub const CROP_REFUSED: &str = "A video can't be cropped.";
/// Why a collage of only videos does not open (until M15.28 takes their first frames).
pub const COLLAGE_REFUSED: &str = "Videos can't go in a collage yet.";

/// The extensions of the videos the scan lists (`formats.md` §1), lowercase.
pub const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "m4v", "mkv"];

/// Whether a file named `name` is a video by its extension.
pub fn is_video_name(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    VIDEO_EXTENSIONS.contains(&ext.as_str())
}

/// Whether photo `id` is a video: the library says so ([`Library::is_video`]) or its name has a
/// video's extension. An unknown id is not.
pub fn is_video(library: &Library, id: PhotoId) -> bool {
    library.photo(id).is_some_and(|p| library.is_video(id) || is_video_name(&p.name))
}

/// Photo `id`'s kind as the commands name it: `"video"` or `"still"`.
pub fn kind_name(library: &Library, id: PhotoId) -> &'static str {
    if is_video(library, id) { "video" } else { "still" }
}

/// `{kind, durationMs}` of photo `id`: `durationMs` the video's length in milliseconds, `null`
/// for a still or a video whose container did not say.
pub fn kind_fields(library: &Library, id: PhotoId) -> Value {
    let duration = library.video_facts(id).map(|f| f.duration_ms).filter(|ms| *ms > 0);
    json!({ "kind": kind_name(library, id), "durationMs": duration })
}

/// Puts [`kind_fields`] into the object `out` (a photo's reply), leaving anything else alone.
pub fn add_kind(out: &mut Value, library: &Library, id: PhotoId) {
    if let (Some(out), Value::Object(fields)) = (out.as_object_mut(), kind_fields(library, id)) {
        out.extend(fields);
    }
}

/// Puts the kind and duration of a file the viewer lists outside the library into the object
/// `out`: `facts` is what its scan read (`None`: a still).
pub fn add_listed_kind(out: &mut Value, facts: Option<&VideoFacts>) {
    let fields = match facts {
        Some(f) => json!({ "kind": PhotoKind::Video, "durationMs": (f.duration_ms > 0).then_some(f.duration_ms) }),
        None => json!({ "kind": PhotoKind::Still, "durationMs": null }),
    };
    if let (Some(out), Value::Object(fields)) = (out.as_object_mut(), fields) {
        out.extend(fields);
    }
}

/// Whether Dalica can play a video of `facts`: `Err` with the reason when it cannot. No decoder
/// yet (M15.15 answers by codec), so every video answers [`CANNOT_PLAY`].
pub fn playable(_facts: &VideoFacts) -> Result<(), &'static str> {
    Err(CANNOT_PLAY)
}

/// The most milliseconds a trim names (a little over 11 days: more than any container lasts).
const MAX_TRIM_MS: u64 = 1_000_000_000;

/// The trim of a recipe: the last `trim(start,end)` entry as milliseconds (`roadmap.md` M15.25),
/// start before end; `None` when there is none or the last one cannot be read.
pub fn trim_of(edits: &[String]) -> Option<(u64, u64)> {
    let entry = edits.iter().rev().find(|e| matches!(EditOp::parse(e), EditOp::Listed { name: "trim", .. }))?;
    let EditOp::Listed { params, .. } = EditOp::parse(entry) else { return None };
    let (start, end) = params.split_once(',')?;
    let ms = |t: &str| t.trim().parse::<u64>().ok().filter(|v| *v <= MAX_TRIM_MS);
    let (start, end) = (ms(start)?, ms(end)?);
    (start < end).then_some((start, end))
}

/// Why `op` may not be added to photo `id`'s recipe, if `id` is a video and `op` is not one of a
/// video's own edits (a rotation, the trim): [`EDITS_REFUSED`]. A still: `None`.
pub fn edit_refusal(library: &Library, id: PhotoId, op: &EditOp) -> Option<&'static str> {
    let own = matches!(op, EditOp::Rotate(_) | EditOp::Listed { name: "trim", .. });
    (!own && is_video(library, id)).then_some(EDITS_REFUSED)
}

/// What a collage says of the videos it left out (`None` when it left none): "1 video left out:
/// videos can't go in a collage yet."
pub fn collage_notice(videos: usize) -> Option<String> {
    match videos {
        0 => None,
        1 => Some("1 video left out: videos can't go in a collage yet.".into()),
        n => Some(format!("{n} videos left out: videos can't go in a collage yet.")),
    }
}

/// Puts `videos` (how many videos a collage left out) and its `notice` ([`collage_notice`]) into
/// the object `out` (`create.collage`'s and `collage.clips.add`'s reply), when it left any out.
pub fn note_collage_videos(out: &mut Value, videos: usize) {
    if videos == 0 {
        return;
    }
    if let Some(out) = out.as_object_mut() {
        out.insert("videos".into(), json!(videos));
        out.insert("notice".into(), json!(collage_notice(videos)));
    }
}

/// `video.inspect`'s reply for video `id` (the caller checked it is one): its path, kind and
/// duration, the container's facts (the stored size, the turn, the size shown, the codecs, the
/// sound, the frame rate in frames per 1 000 s), `playable` with the `reason` when it is not, and
/// its `trim` (`{startMs, endMs}` or `null`).
pub fn inspect(library: &Library, id: PhotoId) -> Option<Value> {
    let photo = library.photo(id)?;
    let folder = library.folder(photo.folder)?;
    let empty = VideoFacts::default();
    let facts = library.video_facts(id).unwrap_or(&empty);
    let (dw, dh) = facts.display_size();
    let play = playable(facts);
    let trim = trim_of(&library.info(id).edits).map(|(start, end)| json!({ "startMs": start, "endMs": end }));
    let mut out = json!({
        "id": id, "name": photo.name, "path": folder.path.join(&photo.name),
        "width": facts.width, "height": facts.height, "rotation": facts.rotation,
        "displayed": { "width": dw, "height": dh },
        "videoCodec": facts.video_codec, "soundCodec": facts.sound_codec, "sound": facts.sound,
        "frameRateMilli": facts.frame_rate_milli,
        "playable": play.is_ok(), "reason": play.err(),
        "trim": trim,
    });
    add_kind(&mut out, library, id);
    Some(out)
}

/// The edit view's video state.
#[derive(Debug, Default)]
pub struct Video {
    open: Option<PhotoId>,
    playing: bool,
    position_ms: u64,
}

impl Video {
    /// The video open in the edit view, if any.
    pub fn open(&self) -> Option<PhotoId> {
        self.open
    }

    /// Whether it is playing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Where it is, in milliseconds from its start.
    pub fn position_ms(&self) -> u64 {
        self.position_ms
    }
}

impl Session {
    /// The edit view's video state.
    pub fn video(&self) -> &Video {
        &self.video
    }

    /// Whether photo `id` is a video ([`is_video`]).
    pub fn is_video(&self, id: PhotoId) -> bool {
        is_video(self.library(), id)
    }
}

#[cfg(test)]
mod tests {
    use super::{collage_notice, is_video_name, trim_of};

    fn edits(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_video_is_known_by_its_extension_in_any_case() {
        assert!(is_video_name("clip.MP4") && is_video_name("a.mov") && is_video_name("b.m4v") && is_video_name("c.mkv"));
        assert!(!is_video_name("a.jpg") && !is_video_name("mp4") && !is_video_name("") && !is_video_name("clip.mp4.jpg"));
    }

    #[test]
    fn the_trim_is_the_last_readable_trim_entry() {
        assert_eq!(trim_of(&edits(&[])), None);
        assert_eq!(trim_of(&edits(&["rotate(90)", "trim(500,1500)"])), Some((500, 1500)));
        assert_eq!(trim_of(&edits(&["trim(1,2)", "rotate(90)", "trim(0,900)"])), Some((0, 900)));
        for bad in ["trim(1500,500)", "trim(5,5)", "trim(a,b)", "trim(1)", "trim(-1,5)", "trim(0,99999999999999999999)", "trim()"] {
            assert_eq!(trim_of(&edits(&["trim(0,10)", bad])), None, "{bad}");
        }
    }

    #[test]
    fn the_collage_notice_counts_the_videos() {
        assert_eq!(collage_notice(0), None);
        assert_eq!(collage_notice(1).as_deref(), Some("1 video left out: videos can't go in a collage yet."));
        assert_eq!(collage_notice(3).as_deref(), Some("3 videos left out: videos can't go in a collage yet."));
    }
}
