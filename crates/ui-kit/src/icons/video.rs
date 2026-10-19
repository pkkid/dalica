//! The glyphs of the video player and the video tab (`editing.md` §11, `viewing.md` §4,
//! `design-system.md` §5): our own drawings on the 20 × 20 grid with the shell icons' pen.
//!
//! They are a set of their own, [`VideoIcon`] ([`paint_video`]), so the other sets' sheets and
//! fingerprints stay as they are. Content stays inside 2 … 18 so the stroke never leaves the
//! square. Meanings the kit already draws are not drawn twice: **Pause** is the slideshow's two
//! bars ([`ViewingIcon::Pause`]); the set names it so the player takes every glyph from one place.
//!
//! The new drawings: **Play** (a bare solid triangle, the transport's, apart from the edit view's
//! Play in a circle that starts a slideshow), **Frame back** and **Frame forward** (an outlined
//! triangle against a thin bar: one frame, apart from the slideshow's solid Previous and Next),
//! **Set Start** and **Set End** (a bracket with a solid triangle pointing into the kept part),
//! **Volume off**, **low** and **high** (one speaker: with a cross, one wave, two waves), and three
//! glyphs that share a strip of film (a frame with sprocket holes along its edges): **Save Frame**
//! (a picture under a row of sprocket holes: a frame taken out as a picture), **Export Clip** (a
//! short strip with an arrow leaving it to the right) and **Video Frames** (a strip of film with a
//! picture in it: the project of saved frames).

use egui::{Color32, Painter, Rect};

use super::pen::Pen;
use super::{Glyph, ViewingIcon};

/// The video glyphs. Keep [`VideoIcon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoIcon {
    /// A solid triangle pointing right: Play.
    Play,
    /// Two upright bars: Pause (the slideshow's [`ViewingIcon::Pause`]).
    Pause,
    /// A thin bar and an outlined triangle pointing left: step one frame back.
    FrameBack,
    /// An outlined triangle pointing right and a thin bar: step one frame forward.
    FrameForward,
    /// An opening bracket and a solid triangle pointing right: Set Start (the trim's start).
    SetStart,
    /// A solid triangle pointing left and a closing bracket: Set End (the trim's end).
    SetEnd,
    /// The speaker with a cross: the sound off (muted).
    VolumeOff,
    /// The speaker with one wave: the sound low.
    VolumeLow,
    /// The speaker with two waves: the sound high.
    VolumeHigh,
    /// A picture under a row of sprocket holes: Save Frame as Picture.
    SaveFrame,
    /// A short strip of film with an arrow leaving it to the right: Export Clip.
    ExportClip,
    /// A strip of film with a picture in it: the Video Frames project.
    VideoFrames,
}

impl VideoIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [VideoIcon; 12] = [
        VideoIcon::Play,
        VideoIcon::Pause,
        VideoIcon::FrameBack,
        VideoIcon::FrameForward,
        VideoIcon::SetStart,
        VideoIcon::SetEnd,
        VideoIcon::VolumeOff,
        VideoIcon::VolumeLow,
        VideoIcon::VolumeHigh,
        VideoIcon::SaveFrame,
        VideoIcon::ExportClip,
        VideoIcon::VideoFrames,
    ];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            VideoIcon::Play => "video-play",
            VideoIcon::Pause => "video-pause",
            VideoIcon::FrameBack => "frame-back",
            VideoIcon::FrameForward => "frame-forward",
            VideoIcon::SetStart => "set-start",
            VideoIcon::SetEnd => "set-end",
            VideoIcon::VolumeOff => "volume-off",
            VideoIcon::VolumeLow => "volume-low",
            VideoIcon::VolumeHigh => "volume-high",
            VideoIcon::SaveFrame => "save-frame",
            VideoIcon::ExportClip => "export-clip",
            VideoIcon::VideoFrames => "video-frames",
        }
    }

    /// The glyph called `name` ([`VideoIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<VideoIcon> {
        VideoIcon::ALL.into_iter().find(|i| i.name() == name)
    }

    /// The glyph of another set this one is, for the meanings the kit already draws.
    pub fn shared(self) -> Option<Glyph> {
        match self {
            VideoIcon::Pause => Some(Glyph::Viewing(ViewingIcon::Pause)),
            _ => None,
        }
    }

    /// The speaker glyph for a volume (0 … 1) and the mute: off when muted or silent, low below
    /// half, high from half up (a volume that is not a number reads as silent).
    pub fn for_volume(level: f32, muted: bool) -> VideoIcon {
        if muted || level.is_nan() || level <= 0.0 {
            VideoIcon::VolumeOff
        } else if level < 0.5 {
            VideoIcon::VolumeLow
        } else {
            VideoIcon::VolumeHigh
        }
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_video(painter: &Painter, rect: Rect, icon: VideoIcon, color: Color32) {
    if let Some(glyph) = icon.shared() {
        super::paint_glyph(painter, rect, glyph, color);
        return;
    }
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

/// One frame forward: an outlined triangle pointing right and a thin bar at its tip (mirror the
/// pen for back).
fn frame_step(p: &Pen) {
    p.closed(&[(4.5, 4.5), (12.0, 10.0), (4.5, 15.5)]);
    p.seg((15.5, 4.5), (15.5, 15.5));
}

/// The trim's start: an opening bracket and a solid triangle pointing right, into the kept part
/// (mirror the pen for the end).
fn trim_mark(p: &Pen) {
    p.elbow(&[(7.5, 3.0), (4.0, 3.0), (4.0, 17.0), (7.5, 17.0)]);
    p.fill(&[(9.0, 6.0), (16.0, 10.0), (9.0, 14.0)]);
}

/// The speaker every volume glyph shares: a small box and the cone opening to the right.
fn speaker(p: &Pen) {
    p.fill(&[(3.5, 8.0), (6.0, 8.0), (6.0, 12.0), (3.5, 12.0)]);
    p.fill(&[(6.0, 8.0), (10.0, 4.5), (10.0, 15.5), (6.0, 12.0)]);
}

/// Sprocket holes: small solid squares along the line `y`, centred on each `x`.
fn sprockets(p: &Pen, y: f32, xs: &[f32]) {
    for &x in xs {
        p.fill(&[(x - 0.9, y - 0.9), (x + 0.9, y - 0.9), (x + 0.9, y + 0.9), (x - 0.9, y + 0.9)]);
    }
}

fn draw(p: &Pen, icon: VideoIcon) {
    match icon {
        VideoIcon::Play => p.fill(&[(5.5, 3.5), (16.5, 10.0), (5.5, 16.5)]),
        // Drawn by the set it is shared with ([`VideoIcon::shared`]).
        VideoIcon::Pause => {}
        VideoIcon::FrameBack => frame_step(&p.mirrored()),
        VideoIcon::FrameForward => frame_step(p),
        VideoIcon::SetStart => trim_mark(p),
        VideoIcon::SetEnd => trim_mark(&p.mirrored()),
        VideoIcon::VolumeOff => {
            speaker(p);
            p.seg((12.5, 7.75), (17.0, 12.25));
            p.seg((17.0, 7.75), (12.5, 12.25));
        }
        VideoIcon::VolumeLow => {
            speaker(p);
            p.arc(10.0, 10.0, 3.5, -50.0, 50.0);
        }
        VideoIcon::VolumeHigh => {
            speaker(p);
            p.arc(10.0, 10.0, 3.5, -50.0, 50.0);
            p.arc(10.0, 10.0, 7.0, -50.0, 50.0);
        }
        VideoIcon::SaveFrame => {
            // A row of sprocket holes over a picture: a frame of the film, taken out.
            sprockets(p, 3.0, &[4.5, 8.2, 11.8, 15.5]);
            p.rect(2.5, 6.5, 17.5, 17.5, 1.0);
            p.fill(&[(4.5, 15.5), (8.5, 10.0), (12.5, 15.5)]);
            p.dot(14.0, 9.8, 1.4);
        }
        VideoIcon::ExportClip => {
            // A short strip (holes along its top and bottom) and an arrow leaving it to the right.
            p.rect(2.5, 4.5, 11.5, 15.5, 1.0);
            sprockets(p, 6.8, &[5.0, 9.0]);
            sprockets(p, 13.2, &[5.0, 9.0]);
            p.seg((7.0, 10.0), (17.5, 10.0));
            p.line(&[(14.5, 7.0), (17.5, 10.0), (14.5, 13.0)]);
        }
        VideoIcon::VideoFrames => {
            // A strip of film: holes along its top and bottom, a picture between them.
            p.rect(2.5, 3.5, 17.5, 16.5, 1.0);
            sprockets(p, 6.0, &[5.5, 10.0, 14.5]);
            sprockets(p, 14.0, &[5.5, 10.0, 14.5]);
            p.fill(&[(6.0, 12.0), (9.0, 8.5), (12.0, 12.0)]);
            p.dot(13.8, 9.6, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in VideoIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `VideoFrames` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(VideoIcon::VideoFrames as usize + 1, VideoIcon::ALL.len());
        let mut names: Vec<_> = VideoIcon::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), VideoIcon::ALL.len(), "names are unique");
        for icon in VideoIcon::ALL {
            assert_eq!(VideoIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(VideoIcon::from_name("play"), None);
    }

    #[test]
    fn the_speaker_follows_the_volume_and_the_mute() {
        assert_eq!(VideoIcon::for_volume(0.8, true), VideoIcon::VolumeOff);
        assert_eq!(VideoIcon::for_volume(0.0, false), VideoIcon::VolumeOff);
        assert_eq!(VideoIcon::for_volume(f32::NAN, false), VideoIcon::VolumeOff);
        assert_eq!(VideoIcon::for_volume(0.3, false), VideoIcon::VolumeLow);
        assert_eq!(VideoIcon::for_volume(0.5, false), VideoIcon::VolumeHigh);
        assert_eq!(VideoIcon::for_volume(f32::INFINITY, false), VideoIcon::VolumeHigh);
    }
}
