//! The face scan (`faces.md` §4, `roadmap.md` M13 → *The scan*): while Find faces in my photos is
//! on and a recogniser is installed, a `faces` job at the lowest priority follows the library's new
//! and changed photos, its pace following the user. A stub until M13.18 fills it: the job, and what
//! the window says of the user's input ([`Session::user_activity`]), so the scan's pace can follow
//! it.

use dalica_core::Timestamp;

use crate::jobs::JobId;
use crate::session::Session;

/// What the window saw of the user since its last report (`faces.md` §4, *Pace follows the
/// user*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserActivity {
    /// Dragging, typing or scrolling: no new photo is started.
    Input,
    /// The pointer moving: one photo at a time.
    Pointer,
    /// Nothing: after three seconds of it, about half the cores.
    Idle,
    /// The window minimised, or idle on the People view: more.
    Away,
}

/// When the window last reported each kind of activity, on the session clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActivitySeen {
    /// The last report of [`UserActivity::Input`].
    pub input: Option<Timestamp>,
    /// The last report of [`UserActivity::Pointer`].
    pub pointer: Option<Timestamp>,
    /// The latest report, whatever it was, and when.
    pub last: Option<(UserActivity, Timestamp)>,
}

/// The face scan's state.
#[derive(Debug, Default)]
pub struct FaceScan {
    job: Option<JobId>,
    seen: ActivitySeen,
}

impl FaceScan {
    /// The `faces` job running or last run ([`crate::jobs::JobKind::Faces`]), if any.
    pub fn job(&self) -> Option<JobId> {
        self.job
    }

    /// What the window has reported of the user's input.
    pub fn activity(&self) -> ActivitySeen {
        self.seen
    }

    fn note(&mut self, activity: UserActivity, now: Timestamp) {
        match activity {
            UserActivity::Input => self.seen.input = Some(now),
            UserActivity::Pointer => self.seen.pointer = Some(now),
            UserActivity::Idle | UserActivity::Away => {}
        }
        self.seen.last = Some((activity, now));
    }
}

impl Session {
    /// The window's report of the user's input (each frame, or whenever it changes), for the face
    /// scan's pace. Only notes the time; it never starts or stops work by itself.
    pub fn user_activity(&mut self, activity: UserActivity) {
        let now = self.clock.now();
        self.face_scan.note(activity, now);
    }

    /// The face scan's state (`faces.md` §4).
    pub fn face_scan(&self) -> &FaceScan {
        &self.face_scan
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use dalica_core::VirtualClock;

    use super::*;

    #[test]
    fn each_report_is_noted_with_the_session_clocks_time() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_000)));
        let mut s = Session::open(dir.path(), clock.clone()).unwrap();
        let t = Timestamp::from_millis;
        assert_eq!(s.face_scan().activity(), ActivitySeen::default());
        s.user_activity(UserActivity::Input);
        clock.advance(Duration::from_millis(500));
        s.user_activity(UserActivity::Pointer);
        clock.advance(Duration::from_millis(500));
        s.user_activity(UserActivity::Idle);
        assert_eq!(
            s.face_scan().activity(),
            ActivitySeen { input: Some(t(1_000)), pointer: Some(t(1_500)), last: Some((UserActivity::Idle, t(2_000))) }
        );
        s.user_activity(UserActivity::Away);
        assert_eq!(s.face_scan().activity().last, Some((UserActivity::Away, t(2_000))));
        assert_eq!((s.face_scan().activity().input, s.face_scan().job()), (Some(t(1_000)), None), "the earlier input is kept; no job started");
    }
}
