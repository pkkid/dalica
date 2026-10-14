//! Search (`organizing.md` §8.1–§8.2): the grammar of the search box, the matcher, and the index
//! that answers a query over a whole library in one pass of set operations.
//!
//! - [`Parsed`] reads the text: words (AND), `-word` (NOT), `tag:` and `camera:`, `person:` and
//!   `color:` (read, matching nothing yet), unknown `name:value` as plain words; tokens split at
//!   punctuation, matched as prefixes ignoring case; date spellings kept whole. Hostile text is
//!   bounded ([`MAX_QUERY`], [`MAX_WORDS`], [`MAX_TERM`]). The grammar's word and field reading
//!   is adapted from LightCraft `crates/catalog/src/query.rs` @ b7091805 (MIT OR Apache-2.0; see
//!   `search/grammar.rs`).
//! - [`Query`] adds the filter buttons ([`Filters`]: Starred, Videos, Geotagged; Faces matches
//!   nothing until M13) and the age slider ([`Age`]); [`Scope`] says what the light table shows
//!   (hidden and small pictures, its sections in order, the clock for Recently Updated).
//! - [`Extract`] is an owned copy of what search reads ([`PhotoDoc`], [`FolderDoc`],
//!   [`AlbumDoc`]); [`Update::for_op`] reads again the records an applied op touched, so an index
//!   kept on a worker follows the library photo by photo.
//! - [`SearchIndex`] answers [`SearchIndex::search`] (the matches in light-table order, the
//!   sections with matches, the counts, folders and bytes the banner shows) and
//!   [`SearchIndex::suggest`] (tags, folders and albums for a prefix, with counts, at most
//!   [`MAX_SUGGESTIONS`]). [`Extract::matching`] applies the matcher's rule ([`text_matches`])
//!   photo by photo: the reference the index is tested against.

mod extract;
mod grammar;
mod index;
mod matcher;

pub use extract::{AlbumDoc, Extract, FolderDoc, PhotoDoc, Update};
pub use grammar::{Atom, Field, MAX_QUERY, MAX_TERM, MAX_WORDS, Parsed, terms};
pub use index::{MAX_SUGGESTIONS, SearchIndex, SearchResult, SectionCount, Suggestion, SuggestionKind};
pub use matcher::{Age, Filters, Query, Scope, text_matches};
