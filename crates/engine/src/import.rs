//! The Import room's session (`organizing.md` §3, `roadmap.md` M11): the source being listed, the
//! listing's generation (a new source drops the older listing's results) and its candidates. It
//! lives in the engine's session, so the room reopens on the last source while it is still there,
//! and the CLI and MCP see the same listing. A stub until M11.7 fills it.

use std::path::{Path, PathBuf};

/// One file of the source that may be imported. M11.7 adds what the listing finds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The file on the source.
    pub path: PathBuf,
}

/// The import session: nothing open until a source is.
#[derive(Debug, Default)]
pub struct ImportSession {
    source: Option<PathBuf>,
    generation: u64,
    candidates: Vec<Candidate>,
}

impl ImportSession {
    /// The folder being listed (a card's `DCIM` folder, a drive's root or a chosen folder).
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// The listing's generation: it rises each time a source is opened.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The candidates listed so far, in capture order.
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }
}
