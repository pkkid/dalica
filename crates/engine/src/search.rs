//! Search mode (`organizing.md` §8, `ui-layout.md` §9): the search field's text, the filters and
//! the age slider, whether the light table shows a search or Find Duplicates' groups, and the last
//! result; and the search worker that answers the queries.
//!
//! - **The worker** (a thread named [`WORKER_THREAD`], started the first time something asks for a
//!   search) keeps a copy of the library and `library`'s [`SearchIndex`] of it. The owner thread
//!   sends it every op it applies ([`Session::search_applied`], called by `Session::apply`); the
//!   worker applies the op to its copy and updates the index for the photos the op touched
//!   ([`Update::for_op`]), so the index is never rebuilt for a change. The copy's revision is
//!   checked against the library's after every op: a copy that went out of step (the library
//!   replaced by a rebuild, an op it refused) is replaced by a fresh copy. The first-indexed times
//!   a scan records after its op (a derived op, which reaches no hook) follow it, read from the
//!   library.
//! - **Queries** are sent after a debounce of [`DEBOUNCE`] on the session clock: every
//!   `search.set` that changes the query starts it again, so five keystrokes inside it run one
//!   query. A change to the library while searching sends the query again once [`DEBOUNCE`] has
//!   passed (not moved on by later changes, so a running scan refreshes the results every
//!   [`DEBOUNCE`] at most). The worker runs only the last of the queries waiting for it. Results
//!   carry the generation of the query they answer (it rises with every change of the text, the
//!   filters or the age); a result of an older generation is dropped. The owner thread does no
//!   search work: it sends, and applies results ([`Session::poll_search`], called by
//!   `Session::poll`).
//! - **Light-table order**: the worker builds the [`Scope`] from its copy with the session's view
//!   options ([`Session::search_tree_options`]; [`scope_of`]), so results come in the order the
//!   light table shows them, the search album lists them in that order, and Save Search as Album
//!   keeps it. The scope's folder tree is kept until the library or the options change.
//! - **Questions that need an answer now** (`search.suggest` for other text, `library.query` with
//!   search text, Save Search as Album before its result arrived) are asked of the worker, and the
//!   owner waits for the answer; [`Session::wait_search`] waits for the pending query the same way
//!   (the CLI, tests).
//!
//! - **People** (`organizing.md` §6.4, `faces.md` §7): beside the text, filters and age the query
//!   holds the People pane's exact filter (`people`, by id: the photos holding a face of every
//!   one of them) and whether people's names are withheld from the match (`withhold_names`, set by
//!   an automation surface without the people permission: [`crate::automation::Policy`]). The
//!   switch belongs to one `search.set`: each call sets it from its own params (left out: names
//!   match), so a window's own search always matches names, while the automation's search, run
//!   again as the library changes, keeps them out.
//!
//! Find Duplicates (M9.16, `duplicates.rs`) shows its groups through the same state in mode
//! [`SearchMode::Duplicates`].

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use dalica_core::{PersonId, PhotoId, Timestamp};
use dalica_library::search::{Age, Filters, MAX_SUGGESTIONS, People, Query, Scope, SearchIndex, SectionCount, Suggestion, Update};
use dalica_library::{AlbumTree, FolderSort, Library, Op, SectionFilter, Tree, TreeMode, TreeOptions};
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::session::Session;

/// How long the search waits after the last change of the query before it runs (`organizing.md`
/// §8.1), on the session clock.
pub const DEBOUNCE: Duration = Duration::from_millis(120);

/// The longest search text a command accepts, in bytes. The grammar reads at most its own cap
/// ([`dalica_library::search::MAX_QUERY`], 1 KiB) of it and reports the rest as `truncated`.
pub const MAX_TEXT: usize = 16 * 1024;

/// The name of the search worker's thread.
pub const WORKER_THREAD: &str = "dalica-search";

/// What the light table shows in search mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchMode {
    /// The matches of the text, filters and age.
    #[default]
    Search,
    /// Find Duplicates' groups, one section each.
    Duplicates,
}

impl SearchMode {
    /// The name `library.inspect` gives it.
    pub fn name(self) -> &'static str {
        match self {
            SearchMode::Search => "search",
            SearchMode::Duplicates => "duplicates",
        }
    }
}

/// A query's answer, tagged with the generation it answers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResult {
    pub generation: u64,
    /// The text it answers (its suggestions are for this text's last word).
    pub text: String,
    /// The matches in light-table order, as the section filter search mode builds the tree with
    /// (`TreeOptions::filter`); [`SearchResult::photos`] lists them.
    pub filter: Arc<SectionFilter>,
    /// Find Duplicates' groups (mode [`SearchMode::Duplicates`]), largest total first.
    pub groups: Vec<Vec<PhotoId>>,
    /// The sections with matches and their counts: the search album first, then the light
    /// table's sections that keep a match.
    pub sections: Vec<SectionCount>,
    /// What the banner shows: the folders the matches are in, and their bytes.
    pub folders: usize,
    pub bytes: u64,
    /// How long the query took on the worker.
    pub seconds: f64,
    /// Operators and filters used that match nothing for want of data: `person` while the library
    /// has no people, `faces` while no photo has a face, `color` (not built yet).
    pub unavailable: Vec<&'static str>,
    /// Whether the text was cut at the grammar's caps.
    pub truncated: bool,
    /// The autocomplete for the text's last word (tags, folders, albums with counts).
    pub suggestions: Vec<Suggestion>,
    /// The thread that ran the query ([`WORKER_THREAD`]).
    pub thread: String,
}

impl SearchResult {
    /// The matches in light-table order.
    pub fn photos(&self) -> &[PhotoId] {
        self.filter.ids()
    }

    /// How many photos match.
    pub fn count(&self) -> usize {
        self.filter.len()
    }
}

/// What the worker did, for tests and benches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Queries run for search mode (one left out for a newer one does not count).
    pub queries: u64,
    /// Questions answered at once (`search.suggest`, `library.query`, Save Search as Album).
    pub asked: u64,
    /// Times the index was built from a fresh copy of the library (the first time, and after the
    /// copy went out of step).
    pub builds: u64,
    /// Ops applied to the copy and the index.
    pub ops: u64,
}

/// Search mode's state.
#[derive(Debug, Default)]
pub struct SearchState {
    /// The search field's text.
    pub text: String,
    pub filters: Filters,
    /// The age slider: photos up to this many days old; `None` means everything.
    pub age_days: Option<u32>,
    /// The People pane's filter: only the photos holding a face of every one of them.
    pub people: Vec<PersonId>,
    /// People's names and nicknames take no part in this search (set per `search.set`).
    pub withhold_names: bool,
    /// `None` while not searching.
    pub mode: Option<SearchMode>,
    /// Rises with every change of the query; a result of an older one is dropped.
    pub generation: u64,
    /// The last result applied.
    pub result: Option<SearchResult>,
    /// When the waiting query is sent, if one waits.
    due: Option<Timestamp>,
    worker: Option<Worker>,
    /// What the session knows of `people.json` (`people_file.rs`), kept here because this
    /// module's hooks follow every op and every poll.
    pub(crate) people_file: crate::people_file::PeopleFile,
}

impl SearchState {
    /// Whether the light table is in search mode.
    pub fn active(&self) -> bool {
        self.mode.is_some()
    }

    /// Whether a query waits for its debounce or for the worker's answer.
    pub fn pending(&self) -> bool {
        self.due.is_some() || self.worker.as_ref().is_some_and(|w| w.in_flight > 0)
    }

    /// What the worker did so far (all zero before it started).
    pub fn stats(&self) -> SearchStats {
        self.worker.as_ref().map(|w| w.stats.get()).unwrap_or_default()
    }

    /// The query of the current text, filters and age, the age counted back from `now`.
    fn query(&self, now: Timestamp) -> Query {
        Query { text: self.text.clone(), filters: self.filters, age: self.age_days.map(|days| Age { days, now }) }
    }

    /// What the current search asks of people.
    fn who(&self) -> People {
        People { people: self.people.clone(), withhold_names: self.withhold_names, ..People::default() }
    }

    /// Whether names are withheld from a match that could hold them: only the text matches names,
    /// so with no text the switch makes no difference.
    fn withholds(&self) -> bool {
        self.withhold_names && !self.text.is_empty()
    }

    /// Whether the current query asks for anything (search mode is on while it does).
    fn asks(&self, now: Timestamp) -> bool {
        self.query(now).is_active() || self.who().is_active()
    }

    /// Enters mode [`SearchMode::Duplicates`] showing `result` (Find Duplicates, `duplicates.rs`):
    /// the text, filters and age are cleared and the generation rises, as for a new query.
    pub(crate) fn enter_duplicates(&mut self, mut result: SearchResult) {
        self.text.clear();
        self.filters = Filters::default();
        self.age_days = None;
        self.people.clear();
        self.withhold_names = false;
        self.mode = Some(SearchMode::Duplicates);
        self.due = None;
        self.generation = self.generation.wrapping_add(1);
        result.generation = self.generation;
        self.result = Some(result);
    }

    /// The last result, when it answers the current query and nothing changed since.
    fn fresh(&self) -> Option<&SearchResult> {
        self.result.as_ref().filter(|r| r.generation == self.generation && !self.pending())
    }
}

/// The scope a search runs in: what the light table shows of `library` with `options` (its
/// sections in order: the albums of [`AlbumTree::build`], then the folders of [`Tree::build`]).
/// The worker builds the same; tests use it to ask `library`'s index the same question.
pub fn scope_of(library: &Library, roots: &[PathBuf], options: &TreeOptions) -> Scope {
    let tree = Tree::build(library, roots, options);
    let albums = AlbumTree::build(library, options);
    Scope::from_options(options, albums.sections_with(&tree))
}

/// The worker's counters.
#[derive(Debug, Default)]
struct Counters {
    queries: AtomicU64,
    asked: AtomicU64,
    builds: AtomicU64,
    ops: AtomicU64,
}

impl Counters {
    fn get(&self) -> SearchStats {
        SearchStats {
            queries: self.queries.load(Ordering::Relaxed),
            asked: self.asked.load(Ordering::Relaxed),
            builds: self.builds.load(Ordering::Relaxed),
            ops: self.ops.load(Ordering::Relaxed),
        }
    }

    fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// One question for the worker: a query and where it runs.
#[derive(Clone, Debug)]
struct Ask {
    query: Query,
    who: People,
    roots: Vec<PathBuf>,
    options: TreeOptions,
    /// Suggestions for the text's last word too.
    suggest: bool,
}

/// What the owner sends the worker.
enum Request {
    /// A fresh copy of the library: the index is built from it.
    Library(Box<Library>),
    /// An op the library applied, and the library's revision after it.
    Op { op: Op, revision: u64 },
    /// A query for search mode; the answer comes back on the replies channel.
    Search { generation: u64, ask: Ask },
    /// A query answered at once on `reply`.
    Answer { ask: Ask, reply: Sender<SearchResult> },
    /// Suggestions for `text`, answered at once on `reply`.
    Suggest { text: String, limit: usize, roots: Vec<PathBuf>, options: TreeOptions, reply: Sender<Vec<Suggestion>> },
}

/// What the worker sends back on the replies channel.
enum Reply {
    Result(SearchResult),
    /// A search left out for a newer one.
    Skipped,
    /// The copy went out of step with the library: send a fresh one.
    Diverged,
}

/// The owner's end of the worker. Dropping it closes the channel and the thread ends.
struct Worker {
    tx: Sender<Request>,
    rx: Receiver<Reply>,
    stats: Arc<Counters>,
    /// Searches sent and not answered yet.
    in_flight: usize,
    /// The library revision the worker's copy is at (as sent); `None` when it needs a fresh copy.
    revision: Option<u64>,
    /// Photos added by the ops sent since the last sync: their first-indexed times follow.
    added: Vec<PhotoId>,
    /// Copies of the library sent so far.
    copies: u64,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker").field("in_flight", &self.in_flight).field("revision", &self.revision).finish_non_exhaustive()
    }
}

/// The worker's own state: the copy of the library, its index, and the folder tree of the last
/// scope.
struct Replica {
    library: Library,
    index: SearchIndex,
    /// The tree of the last scope, with the library revision, roots and options (the clock left
    /// out: the folder tree does not read it) it was built for.
    tree: Option<(u64, Vec<PathBuf>, TreeOptions, Tree)>,
    /// Out of step: ops are ignored until a fresh copy comes.
    diverged: bool,
}

impl Replica {
    fn scope(&mut self, roots: &[PathBuf], options: &TreeOptions) -> Scope {
        let key = TreeOptions { now: None, ..options.clone() };
        let rev = self.library.revision();
        let fits = self.tree.as_ref().is_some_and(|(r, ro, o, _)| *r == rev && ro.as_slice() == roots && *o == key);
        if !fits {
            self.tree = Some((rev, roots.to_vec(), key, Tree::build(&self.library, roots, options)));
        }
        let albums = AlbumTree::build(&self.library, options);
        match &self.tree {
            Some((.., tree)) => Scope::from_options(options, albums.sections_with(tree)),
            None => Scope::from_options(options, Vec::new()),
        }
    }

    fn answer(&mut self, generation: u64, ask: &Ask) -> SearchResult {
        let start = Instant::now();
        let scope = self.scope(&ask.roots, &ask.options);
        let found = self.index.search_with(&ask.query, &ask.who, &scope);
        let seconds = start.elapsed().as_secs_f64();
        let suggestions = if ask.suggest { self.index.suggest(&ask.query.text, &scope, MAX_SUGGESTIONS) } else { Vec::new() };
        SearchResult {
            generation,
            text: ask.query.text.clone(),
            filter: Arc::new(SectionFilter::new(found.ids)),
            groups: Vec::new(),
            sections: found.sections,
            folders: found.folders,
            bytes: found.bytes,
            seconds,
            unavailable: found.unavailable,
            truncated: found.truncated,
            suggestions,
            thread: std::thread::current().name().unwrap_or_default().to_string(),
        }
    }
}

/// The worker's loop: takes every request waiting, runs only the last search among them.
fn work(rx: Receiver<Request>, tx: Sender<Reply>, stats: Arc<Counters>) {
    let mut replica = Replica { library: Library::new(), index: SearchIndex::default(), tree: None, diverged: true };
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(more) = rx.try_recv() {
            batch.push(more);
        }
        let last_search = batch.iter().rposition(|r| matches!(r, Request::Search { .. }));
        for (i, request) in batch.into_iter().enumerate() {
            let outcome = catch_unwind(AssertUnwindSafe(|| handle(&mut replica, request, Some(i) == last_search, &tx, &stats)));
            if outcome.is_err() && !replica.diverged {
                // A panic left the copy in an unknown state: ask for a fresh one.
                log::error!("the search worker caught a panic; it asks for a fresh copy of the library");
                replica.diverged = true;
                let _ = tx.send(Reply::Diverged);
            }
        }
    }
}

fn handle(replica: &mut Replica, request: Request, run: bool, tx: &Sender<Reply>, stats: &Counters) {
    match request {
        Request::Library(library) => {
            replica.index = SearchIndex::from_library(&library);
            replica.library = *library;
            replica.tree = None;
            replica.diverged = false;
            Counters::bump(&stats.builds);
        }
        Request::Op { op, revision } => {
            if replica.diverged {
                return;
            }
            let applied = replica.library.apply(op.clone()).is_ok();
            if applied && replica.library.revision() == revision {
                replica.index.apply(Update::for_op(&replica.library, &op));
                Counters::bump(&stats.ops);
            } else {
                replica.diverged = true;
                let _ = tx.send(Reply::Diverged);
            }
        }
        Request::Search { generation, ask } => {
            if !run {
                let _ = tx.send(Reply::Skipped);
                return;
            }
            Counters::bump(&stats.queries);
            let _ = tx.send(Reply::Result(replica.answer(generation, &ask)));
        }
        Request::Answer { ask, reply } => {
            Counters::bump(&stats.asked);
            let _ = reply.send(replica.answer(0, &ask));
        }
        Request::Suggest { text, limit, roots, options, reply } => {
            Counters::bump(&stats.asked);
            let scope = replica.scope(&roots, &options);
            let _ = reply.send(replica.index.suggest(&text, &scope, limit));
        }
    }
}

/// The photos an op adds (batches walked without recursion).
fn added_by(op: &Op, out: &mut Vec<PhotoId>) {
    let mut stack = vec![op];
    while let Some(op) = stack.pop() {
        match op {
            Op::Batch { ops } => stack.extend(ops.iter()),
            Op::AddPhotos { photos } => out.extend(photos.iter().map(|p| p.id)),
            _ => {}
        }
    }
}

/// 64 random bits from the OS-seeded hasher (new album ids).
pub(crate) fn random_bits() -> u64 {
    RandomState::new().hash_one(Instant::now())
}

impl Session {
    /// Search mode's state.
    pub fn search_state(&self) -> &SearchState {
        &self.search
    }

    /// What the search worker did so far.
    pub fn search_stats(&self) -> SearchStats {
        self.search.stats()
    }

    /// The section filter search mode builds the tree with (`TreeOptions::filter`), while
    /// searching and once a result came.
    pub fn search_filter(&self) -> Option<Arc<SectionFilter>> {
        self.search.mode.and(self.search.result.as_ref()).map(|r| Arc::clone(&r.filter))
    }

    /// The tree options the search orders its results by: the stored view options (tree or flat
    /// mode, the folder sort and its direction, Small and Hidden Pictures), Options → File Types'
    /// small-picture threshold, the projects folder
    /// (unless it is watched itself), the session clock for Recently Updated; every group
    /// expanded.
    pub fn search_tree_options(&self) -> TreeOptions {
        let v = self.settings().view_options();
        let projects_dir = self.projects_dir().filter(|d| !self.roots().contains(d));
        TreeOptions {
            mode: if v.tree_mode.as_deref() == Some("tree") { TreeMode::Tree } else { TreeMode::Flat },
            sort: match v.folder_sort.as_deref() {
                Some("recent") => FolderSort::RecentChanges,
                Some("size") => FolderSort::Size,
                Some("name") => FolderSort::Name,
                _ => FolderSort::Date,
            },
            reverse: v.folder_sort_reverse,
            small_pictures: v.small_pictures,
            small_threshold: self.settings().small_picture_threshold(),
            hidden_pictures: v.hidden_pictures,
            projects_dir,
            now: Some(self.clock.now()),
            ..TreeOptions::default()
        }
    }

    /// `library.inspect` → `search` (`control-protocol.md` §6.4): whether search mode is on and
    /// which, the query (`age` in days, 0 for every age; `people` the People pane's filter), and the banner's numbers once a result
    /// came (`matches` is `null` until then and outside search mode), with the sections that keep a
    /// match by their ids (`album:search`, `folder:12`). In duplicates mode, `groups` lists Find
    /// Duplicates' groups and `banner` is its text (both `null` otherwise; `duplicates.rs`).
    pub fn search_inspect(&self) -> Value {
        let s = &self.search;
        let r = s.result.as_ref().filter(|_| s.active());
        let (groups, banner) = self.duplicates_inspect();
        json!({
            "active": s.active(),
            "mode": s.mode.map(SearchMode::name),
            "text": s.text,
            "filters": s.filters,
            "age": s.age_days.unwrap_or(0),
            "people": s.people,
            "matches": r.map(SearchResult::count),
            "folders": r.map_or(0, |r| r.folders),
            "bytes": r.map_or(0, |r| r.bytes),
            "seconds": r.map_or(0.0, |r| r.seconds),
            "sections": r.map(|r| r.sections.iter().map(|c| json!({ "section": c.section.to_string(), "count": c.count })).collect::<Vec<_>>()),
            "unavailable": r.map(|r| &r.unavailable),
            "truncated": r.is_some_and(|r| r.truncated),
            "pending": s.pending(),
            "generation": s.generation,
            "groups": groups,
            "banner": banner,
        })
    }

    /// Sets the query (`search.set`): `text`, `age_days` and `people` when given, `filters` and
    /// `withhold_names` always. A change starts the debounce again; a query asking for nothing
    /// leaves search mode. Returns whether anything changed. Withholding names shapes only a match
    /// of the text, so with no text the switch alone changes nothing: an automation surface's
    /// `search.set {}` (which always withholds them, `automation::NAME_SEARCHES`) keeps Find
    /// Duplicates' groups on show.
    pub(crate) fn set_search(
        &mut self,
        text: Option<String>,
        filters: Filters,
        age_days: Option<Option<u32>>,
        people: Option<Vec<PersonId>>,
        withhold_names: bool,
    ) -> bool {
        let s = &self.search;
        let before = (s.text.clone(), s.filters, s.age_days, s.people.clone(), s.withholds(), s.mode);
        if let Some(text) = text {
            self.search.text = text;
        }
        self.search.filters = filters;
        if let Some(age) = age_days {
            self.search.age_days = age.filter(|d| *d > 0);
        }
        if let Some(people) = people {
            self.search.people = people;
        }
        self.search.withhold_names = withhold_names;
        if self.search.mode == Some(SearchMode::Duplicates) {
            let s = &self.search;
            if (&s.text, s.filters, s.age_days, &s.people, s.withholds()) == (&before.0, before.1, before.2, &before.3, before.4) {
                // An unchanged query keeps Find Duplicates' groups on show.
                return false;
            }
            // Leaving them: their groups are no search's result.
            self.search.result = None;
        }
        let active = self.search.asks(self.clock.now());
        self.search.mode = active.then_some(SearchMode::Search);
        let s = &self.search;
        let after = (s.text.clone(), s.filters, s.age_days, s.people.clone(), s.withholds(), s.mode);
        if before == after {
            return false;
        }
        self.search.generation = self.search.generation.wrapping_add(1);
        if active {
            self.ensure_search_worker();
            self.search.due = Some(self.clock.now().saturating_add(DEBOUNCE));
        } else {
            self.search.result = None;
            self.search.due = None;
        }
        true
    }

    /// Leaves search mode (`search.clear`): no text, filters or age, and no result.
    pub(crate) fn clear_search(&mut self) {
        self.search.text.clear();
        self.search.filters = Filters::default();
        self.search.age_days = None;
        self.search.people.clear();
        self.search.withhold_names = false;
        self.search.mode = None;
        self.search.result = None;
        self.search.due = None;
        self.search.generation = self.search.generation.wrapping_add(1);
    }

    /// After `op` was applied: the worker's copy and index follow it, and the results of a search
    /// in progress are asked for again once [`DEBOUNCE`] has passed.
    pub(crate) fn search_applied(&mut self, op: &Op) {
        // `people.json` follows every op too (`people_file.rs`).
        self.people_applied(op);
        let revision = self.library.revision();
        let Some(w) = &mut self.search.worker else { return };
        // Each op moves the revision by one (a derived one not at all): anything else means the
        // library changed without an op (a rebuild replaced it), and a fresh copy goes instead.
        let follows = w.revision.is_some_and(|r| r.checked_add(u64::from(!op.is_derived())) == Some(revision));
        if follows {
            added_by(op, &mut w.added);
            let sent = w.tx.send(Request::Op { op: op.clone(), revision }).is_ok();
            w.revision = Some(revision);
            if !sent {
                self.search_worker_stopped();
            }
        } else {
            w.revision = None;
        }
        if self.search.mode == Some(SearchMode::Search) && self.search.due.is_none() {
            self.search.due = Some(self.clock.now().saturating_add(DEBOUNCE));
        }
    }

    /// Applies the search worker's answers (from `poll`), dropping stale ones, after sending the
    /// query whose debounce has passed. Returns how many it applied. Never blocks.
    pub(crate) fn poll_search(&mut self) -> usize {
        // A `people.json` write held while scans applied goes once they are done (`people_file.rs`).
        self.write_people_file();
        if self.search.worker.is_none() {
            return 0;
        }
        self.sync_search_worker();
        if self.search.due.is_some_and(|due| self.clock.now() >= due) {
            self.send_search();
        }
        let mut applied = 0;
        while let Some(reply) = self.search.worker.as_ref().map(|w| w.rx.try_recv()) {
            match reply {
                Ok(reply) => applied += self.take_reply(reply),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.search_worker_stopped();
                    break;
                }
            }
        }
        applied
    }

    /// Sends the waiting query now, whatever the debounce, and blocks until every query sent has
    /// been answered and its result applied (the CLI, tests). Returns how many it applied.
    pub fn wait_search(&mut self) -> usize {
        let mut applied = 0;
        loop {
            if self.search.worker.is_none() {
                self.search.due = None;
                return applied;
            }
            self.sync_search_worker();
            if self.search.due.is_some() {
                self.send_search();
            }
            let reply = match &self.search.worker {
                Some(w) if w.in_flight > 0 => w.rx.recv(),
                _ => return applied,
            };
            match reply {
                Ok(reply) => applied += self.take_reply(reply),
                Err(_) => {
                    self.search_worker_stopped();
                }
            }
        }
    }

    /// The current search's matches in light-table order: the result applied when it answers the
    /// current query and nothing changed since, else asked of the worker now. `None` when not
    /// searching.
    pub(crate) fn search_results_now(&mut self) -> Result<Option<Vec<PhotoId>>, CmdError> {
        if self.search.mode != Some(SearchMode::Search) {
            return Ok(None);
        }
        if let Some(r) = self.search.fresh() {
            return Ok(Some(r.photos().to_vec()));
        }
        let (query, who) = (self.search.query(self.clock.now()), self.search.who());
        Ok(Some(self.ask_search_with(query, who)?.photos().to_vec()))
    }

    /// Runs `query`, with what it asks of people, on the worker and waits for the answer (search
    /// mode is left as it is).
    pub(crate) fn ask_search_with(&mut self, query: Query, who: People) -> Result<SearchResult, CmdError> {
        let who = self.people_for_search(who);
        let (roots, options) = (self.roots().to_vec(), self.search_tree_options());
        let (reply, answer) = mpsc::channel();
        self.ask_worker(Request::Answer { ask: Ask { query, who, roots, options, suggest: false }, reply })?;
        answer.recv().map_err(|_| self.search_worker_stopped())
    }

    /// Suggestions for the last word of `text` (`search.suggest`), at most `limit`: from the last
    /// result when it answers this text and nothing changed since, else asked of the worker now.
    pub(crate) fn search_suggestions(&mut self, text: &str, limit: usize) -> Result<Vec<Suggestion>, CmdError> {
        let limit = limit.min(MAX_SUGGESTIONS);
        if let Some(r) = self.search.fresh().filter(|r| r.text == text) {
            return Ok(r.suggestions.iter().take(limit).cloned().collect());
        }
        let (roots, options) = (self.roots().to_vec(), self.search_tree_options());
        let (reply, answer) = mpsc::channel();
        self.ask_worker(Request::Suggest { text: text.to_string(), limit, roots, options, reply })?;
        answer.recv().map_err(|_| self.search_worker_stopped())
    }

    fn ask_worker(&mut self, request: Request) -> Result<(), CmdError> {
        self.ensure_search_worker();
        self.sync_search_worker();
        let sent = self.search.worker.as_ref().is_some_and(|w| w.tx.send(request).is_ok());
        if sent { Ok(()) } else { Err(self.search_worker_stopped()) }
    }

    /// Starts the worker with a copy of the library, if it is not running.
    fn ensure_search_worker(&mut self) {
        if self.search.worker.is_some() {
            return;
        }
        let (tx, requests) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        let stats = Arc::new(Counters::default());
        let counters = Arc::clone(&stats);
        let started = std::thread::Builder::new().name(WORKER_THREAD.into()).spawn(move || work(requests, replies, counters));
        if let Err(e) = started {
            log::error!("cannot start the search worker: {e}");
            return;
        }
        // The copy goes at the first sync, right after.
        self.search.worker = Some(Worker { tx, rx, stats, in_flight: 0, revision: None, added: Vec::new(), copies: 0 });
        self.sync_search_worker();
    }

    /// Brings the worker's copy in step: the first-indexed times recorded since the last ops sent,
    /// and a fresh copy when it needs one (at the start, and when the library changed without an
    /// op or the copy went out of step).
    fn sync_search_worker(&mut self) {
        let revision = self.library.revision();
        let Some(w) = &mut self.search.worker else { return };
        let mut ok = true;
        if w.revision == Some(revision) && !w.added.is_empty() {
            let indexed: Vec<_> =
                std::mem::take(&mut w.added).into_iter().filter_map(|id| self.library.first_indexed(id).map(|t| (id, Some(t)))).collect();
            if !indexed.is_empty() {
                ok &= w.tx.send(Request::Op { op: Op::SetFirstIndexed { indexed }, revision }).is_ok();
            }
        }
        if w.revision != Some(revision) {
            ok &= w.tx.send(Request::Library(Box::new(self.library.clone()))).is_ok();
            w.revision = Some(revision);
            w.added.clear();
            w.copies += 1;
            // A copy after the first: the search runs again on it.
            if w.copies > 1 && self.search.mode == Some(SearchMode::Search) && self.search.due.is_none() {
                self.search.due = Some(self.clock.now());
            }
        }
        if !ok {
            self.search_worker_stopped();
        }
    }

    /// Sends the current query to the worker.
    fn send_search(&mut self) {
        self.search.due = None;
        if self.search.mode != Some(SearchMode::Search) {
            return;
        }
        let ask = Ask {
            query: self.search.query(self.clock.now()),
            who: self.people_for_search(self.search.who()),
            roots: self.roots().to_vec(),
            options: self.search_tree_options(),
            suggest: !self.search.text.trim().is_empty(),
        };
        let generation = self.search.generation;
        let Some(w) = &mut self.search.worker else { return };
        if w.tx.send(Request::Search { generation, ask }).is_ok() {
            w.in_flight += 1;
        } else {
            self.search_worker_stopped();
        }
    }

    /// Takes one reply; returns 1 when it applied a result.
    fn take_reply(&mut self, reply: Reply) -> usize {
        match reply {
            Reply::Result(r) => {
                if let Some(w) = &mut self.search.worker {
                    w.in_flight = w.in_flight.saturating_sub(1);
                }
                if r.generation == self.search.generation && self.search.mode == Some(SearchMode::Search) {
                    self.search.result = Some(r);
                    1
                } else {
                    0
                }
            }
            Reply::Skipped => {
                if let Some(w) = &mut self.search.worker {
                    w.in_flight = w.in_flight.saturating_sub(1);
                }
                0
            }
            Reply::Diverged => {
                // A fresh copy goes at once; the search runs again on it.
                if let Some(w) = &mut self.search.worker {
                    w.revision = None;
                }
                self.sync_search_worker();
                0
            }
        }
    }

    /// The worker stopped (its thread ended): it starts again with a fresh copy when next needed.
    fn search_worker_stopped(&mut self) -> CmdError {
        log::error!("the search worker stopped");
        self.search.worker = None;
        CmdError::Internal("the search worker stopped".into())
    }
}
