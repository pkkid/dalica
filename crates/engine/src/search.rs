//! Search mode (`organizing.md` §8, `ui-layout.md` §9): the search field's text, the filters and
//! the age slider, whether the light table shows a search or Find Duplicates' groups, and the last
//! result. Queries run on a worker after a debounce on the session clock against `library`'s index,
//! which the worker keeps and updates for the photos each applied op touched; results carry the
//! generation they answer, and stale ones are dropped.
//!
//! The state is here (the session's `search` field); M9.11 fills the worker:
//! [`Session::search_applied`] (called by `Session::apply` after every op) and
//! [`Session::poll_search`] (called by `Session::poll`) do nothing until then. Find Duplicates
//! (M9.16, `duplicates.rs`) shows its groups through the same state in mode
//! [`SearchMode::Duplicates`].

use dalica_core::PhotoId;
use dalica_library::Op;

use crate::session::Session;

/// What the light table shows in search mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchMode {
    /// The matches of the text, filters and age.
    #[default]
    Search,
    /// Find Duplicates' groups, one section each.
    Duplicates,
}

/// The search filters (`organizing.md` §8.2): each on narrows the matches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    pub starred: bool,
    pub videos: bool,
    pub geotagged: bool,
    /// Matches nothing until faces exist (M13).
    pub faces: bool,
}

/// A query's answer, tagged with the generation it answers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResult {
    pub generation: u64,
    /// The matches in light-table order.
    pub photos: Vec<PhotoId>,
    /// Find Duplicates' groups (mode [`SearchMode::Duplicates`]), largest total first.
    pub groups: Vec<Vec<PhotoId>>,
    /// What the banner shows: the folders the matches are in, and their bytes.
    pub folders: usize,
    pub bytes: u64,
    /// How long the query took.
    pub seconds: f64,
}

/// Search mode's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchState {
    /// The search field's text.
    pub text: String,
    pub filters: Filters,
    /// The age slider: photos up to this many days old; `None` means everything.
    pub age_days: Option<u32>,
    /// `None` while not searching.
    pub mode: Option<SearchMode>,
    /// Rises with every change of the query; a result of an older one is dropped.
    pub generation: u64,
    /// The last result applied.
    pub result: Option<SearchResult>,
}

impl SearchState {
    /// Whether the light table is in search mode.
    pub fn active(&self) -> bool {
        self.mode.is_some()
    }
}

impl Session {
    /// Search mode's state.
    pub fn search_state(&self) -> &SearchState {
        &self.search
    }

    /// After `op` was applied: the search worker's index is updated for the photos it touched
    /// (M9.11). Does nothing yet.
    pub(crate) fn search_applied(&mut self, _op: &Op) {}

    /// Applies the search worker's answers (from `poll`), dropping stale ones. Returns how many it
    /// applied; nothing yet (M9.11).
    pub(crate) fn poll_search(&mut self) -> usize {
        0
    }
}
