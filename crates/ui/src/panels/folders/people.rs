//! The People group of the library tree and the person sections (`organizing.md` §6.2–§6.3,
//! `ui-layout.md` §4): a pure function of the library and the view state's people sort, built
//! once per library revision (the tree's model and the light table's sections both read it).
//!
//! - **The group** sits between Albums and Folders, shown while the library has people or
//!   unnamed faces: **Unnamed (n)** first (the faces found and not named), then a row per person
//!   with their thumbnail face and how many photos hold them, ordered by the view options' Sort
//!   People by Name (ignoring case), by Count (most photos first, then by name) or by Top 10 (the
//!   ten people in the most photos by count, then everyone else by name). A person with
//!   suggestions waiting carries the `caution` "?" badge ([`PersonRow::pending`]).
//! - **Person sections** ([`sections`]): every person in that order with a photo that shows (the
//!   tree options' small and hidden pictures and search filter apply, as to an album), between the
//!   albums' sections and the folders'; none while the group is collapsed.
//! - **The face scan** ([`scan_title`]): while the scan runs its header reads the scan's own
//!   progress line, "Finding faces… 312 of 4,980" (`dalica_engine::faces_scan::progress_label`), in
//!   place of "People (3)".

use dalica_core::{PersonId, PhotoId};
use dalica_engine::{JobState, Session};
use dalica_library::{Library, PeopleSort as LibSort, Section as LibSection, TreeOptions, section_photos};

use crate::state::{FaceRef, PeopleSort, PersonKey};

/// How many people Sort People by Top 10 puts first.
pub const TOP: usize = 10;
/// The Unnamed row's label.
pub const UNNAMED: &str = "Unnamed";

/// A person's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonRow {
    pub id: PersonId,
    pub key: PersonKey,
    pub name: String,
    /// The photos holding their faces.
    pub count: usize,
    /// Their thumbnail face (Set as Person Thumbnail's, else their first).
    pub face: Option<FaceRef>,
    /// Suggestions waiting for them (the "?" badge; [`PeopleGroup::set_pending`]).
    pub pending: usize,
}

/// The People group as the tree shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeopleGroup {
    pub collapsed: bool,
    /// The faces in Unnamed.
    pub unnamed: usize,
    /// Every person, in the sort's order (empty while collapsed).
    pub rows: Vec<PersonRow>,
    /// How many people there are (the header's count, also while collapsed).
    pub people: usize,
}

impl PeopleGroup {
    /// The group of `library` in `sort`'s order; its rows left out when `collapsed`.
    pub fn build(library: &Library, sort: PeopleSort, collapsed: bool) -> PeopleGroup {
        let people = library.people().count();
        let unnamed = library.unnamed_count();
        if collapsed {
            return PeopleGroup { collapsed, unnamed, rows: Vec::new(), people };
        }
        let rows = order(library, sort)
            .into_iter()
            .filter_map(|id| {
                let p = library.person(&id)?;
                let face = library.person_thumbnail(&id).and_then(|f| Some(FaceRef { photo: f.photo, index: u16::try_from(f.index).ok()? }));
                Some(PersonRow { key: PersonKey::of(&id), name: p.name.clone(), count: library.person_count(&id), face, pending: 0, id })
            })
            .collect();
        PeopleGroup { collapsed, unnamed, rows, people }
    }

    /// Lights each person's "?" badge from `pending`, the suggestions waiting per person.
    pub fn set_pending(&mut self, pending: &std::collections::BTreeMap<PersonId, usize>) {
        for row in &mut self.rows {
            row.pending = pending.get(&row.id).copied().unwrap_or(0);
        }
    }

    /// Whether the group shows at all: the library has people or unnamed faces.
    pub fn is_empty(&self) -> bool {
        self.people == 0 && self.unnamed == 0
    }
}

/// The people of `library` in `sort`'s order.
pub fn order(library: &Library, sort: PeopleSort) -> Vec<PersonId> {
    match sort {
        PeopleSort::Name => library.people_sorted(LibSort::Name),
        PeopleSort::Count => library.people_sorted(LibSort::Count),
        PeopleSort::Top10 => {
            let mut top: Vec<PersonId> = library.people_sorted(LibSort::Count).into_iter().take(TOP).collect();
            top.retain(|id| library.person_count(id) > 0);
            let rest: Vec<PersonId> = library.people_sorted(LibSort::Name).into_iter().filter(|id| !top.contains(id)).collect();
            top.extend(rest);
            top
        }
    }
}

/// The person sections of `library` under `options`, in `sort`'s order: each person with a photo
/// that shows, with their name and those photos (by date, like a folder); none while the People
/// group is collapsed.
pub fn sections(library: &Library, options: &TreeOptions, sort: PeopleSort) -> Vec<(PersonKey, String, Vec<PhotoId>)> {
    if options.collapsed_groups.contains(&dalica_library::Group::People) {
        return Vec::new();
    }
    let people = order(library, sort);
    let wanted: Vec<LibSection> = people.iter().filter(|id| library.person_count(id) > 0).cloned().map(LibSection::Person).collect();
    section_photos(library, &wanted, options)
        .into_iter()
        .filter_map(|(section, photos)| {
            let LibSection::Person(id) = section else { return None };
            let name = library.person(&id)?.name.clone();
            (!photos.is_empty()).then(|| (PersonKey::of(&id), name, photos))
        })
        .collect()
}

/// The People group's header while the face scan runs, if it does: the scan's progress line,
/// "Finding faces… 312 of 4,980".
pub fn scan_title(session: &Session) -> Option<String> {
    let job = session.job(session.face_scan().job()?)?;
    let running = matches!(job.state, JobState::Running | JobState::Queued) && job.progress.total > 0;
    running.then(|| dalica_engine::faces_scan::progress_label(job.progress.done.min(job.progress.total), job.progress.total))
}
