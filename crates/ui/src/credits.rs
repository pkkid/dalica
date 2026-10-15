//! The About window's credits (`contributors.md`): every contributor (the GitHub username always;
//! a display or real name only from a verified `people.toml` entry) and the AI models named in
//! `Co-Authored-By` trailers. `build.rs` bakes the tables into the binary from
//! `contributors/contributors.json` and `contributors/people.toml`; nothing is read at run time.
//!
//! Adapted from PhotoCraft `crates/ui-egui/src/credits.rs` (revision 184c226a); MIT OR Apache-2.0.
//! Dalica's version keeps its view in [`UiState::about`](crate::state::UiState::about) (so the
//! control channel drives it), draws its tables with `ui-kit` tokens instead of `egui_extras`, and
//! registers every control by its widget id (`control-protocol.md` §7):
//!
//! - the name toggle `choice:dialog.names/<username|displayName|realName>`;
//! - the sort `choice:dialog.sort` (a dropdown whose rows are `choice:dialog.sort/<sort>`) and
//!   `button:dialog.reverse`;
//! - the list `choice:dialog.list/<grabBag|table>`, and in the table the headers
//!   `button:dialog.sort/<sort>` (a click sorts by that column, again reverses it).
//!
//! What the dialogs show of the build ([`Baked`]: version, commit, build date, the credits and the
//! release notes) can be replaced on one egui context ([`set_baked`]), so tests and fingerprints
//! draw fixtures that do not change with every refresh.

use std::cmp::Ordering;

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Segment, SegmentLabel, Target};
use dalica_ui_kit::{Icon, Tokens, fonts};
use egui::{Align2, Id, Label, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::state::{About, CreditsList, CreditsSort, NameMode};

/// One contributor: GitHub username, chosen names and commit stats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contributor {
    pub login: &'static str,
    pub display_name: Option<&'static str>,
    pub real_name: Option<&'static str>,
    pub prs: u64,
    pub commits: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub binary_added: u64,
    pub binary_deleted: u64,
    /// ISO 8601 UTC; empty for someone credited only through a merged pull request.
    pub first_commit: &'static str,
    pub last_commit: &'static str,
}

/// One AI model of the `Co-Authored-By` trailers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub company: &'static str,
    pub model: &'static str,
    pub version: &'static str,
    pub commits: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
}

/// The tables `build.rs` generated: `TOTAL_COMMITS`, `CONTRIBUTORS` and `MODELS`.
pub mod compiled {
    use super::{Contributor, Model};
    include!(concat!(env!("OUT_DIR"), "/credits.rs"));

    /// The generated source itself, for the test that no email address reaches the binary.
    pub const SOURCE: &str = include_str!(concat!(env!("OUT_DIR"), "/credits.rs"));
}

/// What the About and Release Notes dialogs show of this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Baked {
    pub version: &'static str,
    /// The full commit, when the build recorded it (`DALICA_BUILD_SHA`).
    pub commit: Option<&'static str>,
    /// The build date, when the build recorded it (`DALICA_BUILD_DATE`).
    pub build_date: Option<&'static str>,
    pub total_commits: u64,
    pub contributors: &'static [Contributor],
    pub models: &'static [Model],
    /// `whats-new.md`, as Help ▸ Release Notes shows it.
    pub release_notes: &'static str,
}

impl Baked {
    /// What this binary carries.
    pub fn compiled() -> Baked {
        Baked {
            version: dalica_core::build::VERSION,
            commit: dalica_core::build::commit(),
            build_date: dalica_core::build::build_date(),
            total_commits: compiled::TOTAL_COMMITS,
            contributors: compiled::CONTRIBUTORS,
            models: compiled::MODELS,
            release_notes: crate::dialogs::release_notes::NOTES,
        }
    }
}

fn baked_id() -> Id {
    Id::new("dalica.about.baked")
}

/// What the dialogs drawn on `ctx` show: [`Baked::compiled`] unless [`set_baked`] replaced it.
pub fn baked(ctx: &egui::Context) -> Baked {
    ctx.data(|d| d.get_temp::<Baked>(baked_id())).unwrap_or_else(Baked::compiled)
}

/// Makes the dialogs drawn on `ctx` show `baked` instead of this build's data (`None`: this
/// build's again). For tests and fingerprints, whose pictures must not change with each refresh of
/// the credits, each release-notes entry or each version.
pub fn set_baked(ctx: &egui::Context, baked: Option<Baked>) {
    ctx.data_mut(|d| match baked {
        Some(b) => {
            d.insert_temp(baked_id(), b);
        }
        None => d.remove::<Baked>(baked_id()),
    });
}

// ---------------------------------------------------------------------------------------------
// Names and sorting
// ---------------------------------------------------------------------------------------------

/// The name toggle's modes: their state value, widget id part and label.
pub const NAME_MODES: [(NameMode, &str, &str); 3] = [
    (NameMode::Username, "username", "Username"),
    (NameMode::DisplayName, "displayName", "Display name"),
    (NameMode::RealName, "realName", "Real name"),
];

/// The sorts: their state value, widget id part, long label (the dropdown, a header's tooltip) and
/// table header, in column order.
pub const SORTS: [(CreditsSort, &str, &str, &str); 10] = [
    (CreditsSort::Name, "name", "Name (A–Z)", "Name"),
    (CreditsSort::Prs, "prs", "Merged pull requests", "PRs"),
    (CreditsSort::Commits, "commits", "Commits", "Commits"),
    (CreditsSort::LinesAdded, "linesAdded", "Lines added", "+Lines"),
    (CreditsSort::LinesDeleted, "linesDeleted", "Lines deleted", "−Lines"),
    (CreditsSort::LinesDelta, "linesDelta", "Line delta", "ΔLines"),
    (CreditsSort::BinaryAdded, "binaryAdded", "Binary assets added", "+Bin"),
    (CreditsSort::BinaryDeleted, "binaryDeleted", "Binary assets removed", "−Bin"),
    (CreditsSort::FirstCommit, "firstCommit", "First commit", "First"),
    (CreditsSort::LastCommit, "lastCommit", "Last commit", "Last"),
];

/// The lists: their state value, widget id part and label.
pub const LISTS: [(CreditsList, &str, &str); 2] = [(CreditsList::GrabBag, "grabBag", "Grab bag"), (CreditsList::Table, "table", "Table")];

fn sort_row(sort: CreditsSort) -> (CreditsSort, &'static str, &'static str, &'static str) {
    SORTS.iter().copied().find(|s| s.0 == sort).unwrap_or(SORTS[0])
}

/// The order a sort starts in when chosen: names and dates read A or oldest first, counts biggest
/// first.
pub fn default_ascending(sort: CreditsSort) -> bool {
    matches!(sort, CreditsSort::Name | CreditsSort::FirstCommit | CreditsSort::LastCommit)
}

/// Counts line up on the right of their column; names and dates read from the left.
fn numeric(sort: CreditsSort) -> bool {
    !default_ascending(sort)
}

impl Contributor {
    /// The name shown in `mode`; a missing display or real name falls back to `@username`.
    pub fn name(&self, mode: NameMode) -> String {
        let chosen = match mode {
            NameMode::Username => None,
            NameMode::DisplayName => self.display_name,
            NameMode::RealName => self.real_name,
        };
        chosen.map_or_else(|| format!("@{}", self.login), str::to_string)
    }

    pub fn lines_delta(&self) -> i128 {
        i128::from(self.lines_added) - i128::from(self.lines_deleted)
    }

    /// One line with everything known, for the tooltip.
    pub fn summary(&self) -> String {
        format!(
            "@{}: {} merged pull requests, {} commits, +{} / −{} lines (Δ {}), +{} / −{} binary assets, {} – {}",
            self.login,
            group(self.prs),
            group(self.commits),
            group(self.lines_added),
            group(self.lines_deleted),
            signed(self.lines_delta()),
            group(self.binary_added),
            group(self.binary_deleted),
            day(self.first_commit),
            day(self.last_commit),
        )
    }

    /// Its cell in the column of `sort` (the name column shows [`Contributor::name`]).
    fn cell(&self, sort: CreditsSort, mode: NameMode) -> String {
        match sort {
            CreditsSort::Name => self.name(mode),
            CreditsSort::Prs => group(self.prs),
            CreditsSort::Commits => group(self.commits),
            CreditsSort::LinesAdded => group(self.lines_added),
            CreditsSort::LinesDeleted => group(self.lines_deleted),
            CreditsSort::LinesDelta => signed(self.lines_delta()),
            CreditsSort::BinaryAdded => group(self.binary_added),
            CreditsSort::BinaryDeleted => group(self.binary_deleted),
            CreditsSort::FirstCommit => day(self.first_commit).to_string(),
            CreditsSort::LastCommit => day(self.last_commit).to_string(),
        }
    }
}

/// The alphabetical key: case-insensitive, ignoring the `@` of usernames.
fn name_key(name: &str) -> String {
    name.trim_start_matches('@').to_lowercase()
}

fn compare(a: &Contributor, b: &Contributor, mode: NameMode, sort: CreditsSort) -> Ordering {
    match sort {
        CreditsSort::Name => name_key(&a.name(mode)).cmp(&name_key(&b.name(mode))),
        CreditsSort::Prs => a.prs.cmp(&b.prs),
        CreditsSort::Commits => a.commits.cmp(&b.commits),
        CreditsSort::LinesAdded => a.lines_added.cmp(&b.lines_added),
        CreditsSort::LinesDeleted => a.lines_deleted.cmp(&b.lines_deleted),
        CreditsSort::LinesDelta => a.lines_delta().cmp(&b.lines_delta()),
        CreditsSort::BinaryAdded => a.binary_added.cmp(&b.binary_added),
        CreditsSort::BinaryDeleted => a.binary_deleted.cmp(&b.binary_deleted),
        // Someone with no commit date (credited through a pull request) sorts after every date.
        CreditsSort::FirstCommit => dated(a.first_commit).cmp(&dated(b.first_commit)),
        CreditsSort::LastCommit => dated(a.last_commit).cmp(&dated(b.last_commit)),
    }
}

fn dated(d: &str) -> (bool, &str) {
    (d.is_empty(), d)
}

/// The contributors in the order `view` shows them. Ties fall back to the name (A–Z), then the
/// username, so the order is stable.
pub fn sorted<'a>(list: &'a [Contributor], view: &About) -> Vec<&'a Contributor> {
    let mut v: Vec<&Contributor> = list.iter().collect();
    v.sort_by(|a, b| {
        let o = compare(a, b, view.names, view.sort);
        let o = if view.ascending { o } else { o.reverse() };
        o.then_with(|| compare(a, b, view.names, CreditsSort::Name)).then_with(|| a.login.cmp(b.login))
    });
    v
}

/// The names About ▸ Contributors lists for `view`, in its order (the grab bag's and the table's).
pub fn listed(list: &[Contributor], view: &About) -> Vec<String> {
    sorted(list, view).into_iter().map(|c| c.name(view.names)).collect()
}

/// `n` with thousands separators: `1,234,567`.
pub fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `n` things: `1 commit`, `1,200 commits`.
pub fn count(n: u64, thing: &str) -> String {
    if n == 1 { format!("1 {thing}") } else { format!("{} {thing}s", group(n)) }
}

/// A signed count: `+1,000`, `−1,000`.
pub fn signed(n: i128) -> String {
    let g = group(u64::try_from(n.unsigned_abs()).unwrap_or(u64::MAX));
    if n < 0 { format!("−{g}") } else { format!("+{g}") }
}

/// The day of an ISO 8601 time, `—` when there is none.
pub fn day(iso: &str) -> &str {
    if iso.is_empty() { "—" } else { iso.get(..10).unwrap_or(iso) }
}

/// A share of all commits in whole percent; a model with any commits never reads `0%`.
pub fn percent(commits: u64, total: u64) -> String {
    let p = 100.0 * commits as f64 / total.max(1) as f64;
    if commits > 0 && p < 0.5 { "<1%".into() } else { format!("{:.0}%", p.min(100.0)) }
}

/// The Models table's cells for `m`: company, model, version, commits, share, lines.
pub fn model_cells(m: &Model, total: u64) -> [String; 6] {
    [
        m.company.to_string(),
        m.model.to_string(),
        m.version.to_string(),
        group(m.commits),
        percent(m.commits, total),
        format!("+{} / −{}", group(m.lines_added), group(m.lines_deleted)),
    ]
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

const HEADER_H: f32 = 24.0;
const ROW_H: f32 = 22.0;
/// Room for the sort arrow in every header, so no column moves when the sort does.
const ARROW_W: f32 = 12.0;
/// Padding inside each cell, on each side.
const PAD: f32 = 6.0;
/// The narrowest the flexible column (names, models) gets.
const FLEX_MIN: f32 = 110.0;

/// About ▸ Contributors: the name toggle, the sort and the list as a grab bag or a table, in
/// `height` points. Changes go to `view`.
pub fn contributors_ui(ui: &mut Ui, t: &Tokens, baked: &Baked, view: &mut About, height: f32) {
    let top = ui.cursor().min.y;
    controls(ui, t, view);
    ui.add_space(6.0);
    let counts = format!("{} · {}", count(baked.contributors.len() as u64, "contributor"), count(baked.total_commits, "commit"));
    ui.label(RichText::new(counts).font(fonts::regular(t.small)).color(t.text_dim));
    ui.add_space(6.0);
    let rest = (height - (ui.cursor().min.y - top)).max(ROW_H * 3.0);
    if baked.contributors.is_empty() {
        ui.label(RichText::new("No contributor data was built into this copy.").font(fonts::regular(t.body)).color(t.text_label));
        return;
    }
    let list = sorted(baked.contributors, view);
    match view.list {
        CreditsList::GrabBag => grab_bag(ui, t, &list, view.names, rest),
        CreditsList::Table => {
            let mode = view.names;
            let columns: Vec<Column<'_>> = SORTS
                .iter()
                .map(|&(sort, part, long, short)| Column {
                    head: short,
                    tip: long,
                    numeric: numeric(sort),
                    flex: sort == CreditsSort::Name,
                    id: Some(format!("button:dialog.sort/{part}")),
                    sorted: (view.sort == sort).then_some(view.ascending),
                })
                .collect();
            let rows: Vec<Vec<String>> = list.iter().map(|c| SORTS.iter().map(|s| c.cell(s.0, mode)).collect()).collect();
            let tips: Vec<String> = list.iter().map(|c| c.summary()).collect();
            if let Some(i) = table(ui, t, "dialog:about/contributors", &columns, &rows, &tips, rest)
                && let Some(&(sort, ..)) = SORTS.get(i)
            {
                choose_sort(view, sort);
            }
        }
    }
}

/// Picks `sort`: the same one again reverses the order, another starts in its natural order.
fn choose_sort(view: &mut About, sort: CreditsSort) {
    if view.sort == sort {
        view.ascending = !view.ascending;
    } else {
        view.sort = sort;
        view.ascending = default_ascending(sort);
    }
}

/// The row of the name toggle, the sort and the list choice.
fn controls(ui: &mut Ui, t: &Tokens, view: &mut About) {
    let size = vec2(60.0, t.metrics.button_h);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Show").font(fonts::regular(t.body)).color(t.text_label));
        let ids: Vec<String> = NAME_MODES.iter().map(|m| format!("choice:dialog.names/{}", m.1)).collect();
        let segments: Vec<Segment<'_>> =
            NAME_MODES.iter().zip(&ids).map(|(m, id)| Segment { id, label: SegmentLabel::Text(m.2), tooltip: "" }).collect();
        let on = NAME_MODES.iter().position(|m| m.0 == view.names);
        if let Some(m) = widgets::segmented(ui, t, &segments, on, size).and_then(|i| NAME_MODES.get(i)) {
            view.names = m.0;
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Sort").font(fonts::regular(t.body)).color(t.text_label));
        let mut chosen = None;
        widgets::dropdown(ui, t, "choice:dialog.sort", sort_row(view.sort).2, |ui| {
            ui.set_min_width(190.0);
            for (sort, part, long, _) in SORTS {
                let on = view.sort == sort;
                let r = ui.add(egui::Button::selectable(on, long));
                widgets::register(ui.ctx(), &format!("choice:dialog.sort/{part}"), Role::MenuItem, long, Target::Widget(&r), on);
                if r.clicked() {
                    chosen = Some(sort);
                    ui.close();
                }
            }
        });
        if let Some(sort) = chosen {
            view.sort = sort;
            view.ascending = default_ascending(sort);
        }
        let (icon, tip) =
            if view.ascending { (Icon::ChevronUp, "Ascending: reverse the order") } else { (Icon::ChevronDown, "Descending: reverse the order") };
        if widgets::icon_button(ui, t, "button:dialog.reverse", icon, t.metrics.button_h, false, true, tip).clicked() {
            view.ascending = !view.ascending;
        }
        ui.add_space(12.0);
        let ids: Vec<String> = LISTS.iter().map(|l| format!("choice:dialog.list/{}", l.1)).collect();
        let segments: Vec<Segment<'_>> = LISTS.iter().zip(&ids).map(|(l, id)| Segment { id, label: SegmentLabel::Text(l.2), tooltip: "" }).collect();
        let on = LISTS.iter().position(|l| l.0 == view.list);
        if let Some(l) = widgets::segmented(ui, t, &segments, on, size).and_then(|i| LISTS.get(i)) {
            view.list = l.0;
        }
    });
}

/// The names flowing as text, each with its summary as a tooltip, scrolling within `height`.
fn grab_bag(ui: &mut Ui, t: &Tokens, list: &[&Contributor], mode: NameMode, height: f32) {
    egui::ScrollArea::vertical()
        .id_salt("dialog:about/grabBag")
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| {
            // Spacing alone separates the names; each name is one unit that never wraps inside.
            ui.spacing_mut().item_spacing = vec2(14.0, 8.0);
            ui.spacing_mut().interact_size.y = 0.0;
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.horizontal_wrapped(|ui| {
                for c in list {
                    let name = c.name(mode);
                    let r =
                        ui.add(Label::new(RichText::new(&name).font(fonts::regular(t.body)).color(t.text)).selectable(false).sense(Sense::hover()));
                    widgets::tooltip(r, t, &name, Some(&c.summary()));
                }
            });
        });
}

/// About ▸ Models: the models of the `Co-Authored-By` trailers, within `height` points.
pub fn models_ui(ui: &mut Ui, t: &Tokens, baked: &Baked, height: f32) {
    ui.label(RichText::new("AI models credited in the Co-Authored-By trailers of Dalica's commits.").font(fonts::regular(t.small)).color(t.text_dim));
    ui.add_space(6.0);
    if baked.models.is_empty() {
        ui.label(RichText::new("No model credits were built into this copy.").font(fonts::regular(t.body)).color(t.text_label));
        return;
    }
    // Counted against every commit, or the busiest model's if the total somehow trails it.
    let total = baked.total_commits.max(baked.models.iter().map(|m| m.commits).max().unwrap_or(0));
    let heads = ["Company", "Model", "Version", "Commits", "Share of commits", "Lines +/−"];
    let columns: Vec<Column<'_>> =
        heads.iter().enumerate().map(|(i, h)| Column { head: h, tip: "", numeric: i >= 3, flex: i == 1, id: None, sorted: None }).collect();
    let rows: Vec<Vec<String>> = baked.models.iter().map(|m| model_cells(m, total).to_vec()).collect();
    let fit = HEADER_H + rows.len() as f32 * ROW_H + 2.0;
    table(ui, t, "dialog:about/models", &columns, &rows, &[], fit.min(height - 26.0).max(HEADER_H + ROW_H));
}

/// A table column.
struct Column<'a> {
    head: &'a str,
    /// The header's tooltip (empty for none).
    tip: &'a str,
    numeric: bool,
    /// Takes the width the others leave (at least [`FLEX_MIN`]).
    flex: bool,
    /// The header's widget id, for a column that sorts on a click.
    id: Option<String>,
    /// `Some(ascending)` for the column sorted by.
    sorted: Option<bool>,
}

/// A table of `rows` (one cell per column; the first column in `text`, the others `text_label`)
/// under a header that stays put while the rows scroll within `height` (header included). `tips`
/// are the rows' tooltips, if any. Returns the index of the header clicked.
fn table(ui: &mut Ui, t: &Tokens, salt: &str, columns: &[Column<'_>], rows: &[Vec<String>], tips: &[String], height: f32) -> Option<usize> {
    let cell_font = fonts::regular(t.body);
    let head_font = fonts::semibold(t.body);
    let text_w = |ui: &Ui, s: &str, f: &egui::FontId| ui.painter().layout_no_wrap(s.to_owned(), f.clone(), t.text).size().x;
    let fixed: Vec<f32> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if c.flex {
                return 0.0;
            }
            let widest = rows.iter().filter_map(|r| r.get(i)).map(|s| text_w(ui, s, &cell_font)).fold(0.0, f32::max);
            let head = text_w(ui, c.head, &head_font) + if c.id.is_some() { ARROW_W } else { 0.0 };
            (widest.max(head) + 2.0 * PAD).ceil()
        })
        .collect();
    let bar = ui.spacing().scroll.allocated_width();
    let full = ui.available_width();
    let flex = (full - bar - fixed.iter().sum::<f32>()).max(FLEX_MIN).floor();
    let widths: Vec<f32> = columns.iter().zip(&fixed).map(|(c, w)| if c.flex { flex } else { *w }).collect();
    let mut clicked = None;
    // No gaps: the header sits on the rows, which sit on each other.
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    // The header.
    let (head, _) = ui.allocate_exact_size(vec2(full, HEADER_H), Sense::hover());
    let clip = head.intersect(ui.clip_rect());
    let mut x = head.min.x;
    for (i, (c, w)) in columns.iter().zip(&widths).enumerate() {
        let rect = Rect::from_min_size(pos2(x, head.min.y), vec2(*w, HEADER_H));
        x += w;
        let hovered = if let Some(id) = &c.id {
            let r = ui.interact(rect.intersect(clip), Id::new(id), Sense::click());
            widgets::register(ui.ctx(), id, Role::Button, c.tip, Target::Widget(&r), c.sorted.is_some());
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, c.tip));
            let hovered = r.hovered();
            if r.clicked() {
                clicked = Some(i);
            }
            widgets::tooltip(r, t, c.tip, None);
            hovered
        } else {
            false
        };
        let color = if c.sorted.is_some() || hovered { t.text } else { t.text_label };
        let p = ui.painter().with_clip_rect(clip);
        let galley = p.layout_no_wrap(c.head.to_owned(), head_font.clone(), color);
        let inner = rect.shrink2(vec2(PAD, 0.0));
        let y = rect.center().y - galley.size().y / 2.0;
        let (text_x, arrow_x) = if c.numeric {
            let x = inner.right() - galley.size().x;
            (x, x - ARROW_W / 2.0)
        } else {
            (inner.left(), inner.left() + galley.size().x + ARROW_W / 2.0)
        };
        p.galley(pos2(text_x, y).round(), galley, color);
        if let Some(ascending) = c.sorted {
            let (m, h) = (pos2(arrow_x, rect.center().y), 3.0);
            let pts = if ascending {
                vec![m + vec2(-h, h * 0.6), m + vec2(h, h * 0.6), m + vec2(0.0, -h * 0.9)]
            } else {
                vec![m + vec2(-h, -h * 0.6), m + vec2(h, -h * 0.6), m + vec2(0.0, h * 0.9)]
            };
            p.add(egui::Shape::convex_polygon(pts, t.accent, egui::Stroke::NONE));
        }
    }
    ui.painter().with_clip_rect(clip).hline(head.x_range(), head.max.y - 0.5, egui::Stroke::new(1.0, t.button_border));
    // The rows.
    let body_h = (height - HEADER_H).max(ROW_H);
    ui.scope(|ui| {
        egui::ScrollArea::vertical()
            .id_salt(salt)
            .max_height(body_h)
            .min_scrolled_height(body_h)
            .auto_shrink([false, false])
            .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
            .show_rows(ui, ROW_H, rows.len(), |ui, range| {
                for i in range {
                    let Some(row) = rows.get(i) else { break };
                    let (rect, r) = ui.allocate_exact_size(vec2(full - bar, ROW_H), Sense::hover());
                    let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                    if i % 2 == 1 {
                        p.rect_filled(rect, 0, t.inset);
                    }
                    let mut x = rect.min.x;
                    for (j, ((c, w), s)) in columns.iter().zip(&widths).zip(row).enumerate() {
                        let cell = Rect::from_min_size(pos2(x, rect.min.y), vec2(*w, ROW_H));
                        x += w;
                        let color = if j == 0 { t.text } else { t.text_label };
                        let inner = cell.shrink2(vec2(PAD, 0.0));
                        let (anchor, at) = if c.numeric {
                            (Align2::RIGHT_CENTER, pos2(inner.right(), cell.center().y))
                        } else {
                            (Align2::LEFT_CENTER, pos2(inner.left(), cell.center().y))
                        };
                        p.with_clip_rect(cell.intersect(p.clip_rect())).text(at, anchor, s, cell_font.clone(), color);
                    }
                    if let (Some(tip), Some(name)) = (tips.get(i), row.first()) {
                        widgets::tooltip(r, t, name, Some(tip));
                    }
                }
            });
    });
    clicked
}

/// `build.rs`, compiled into the unit tests to test its parser and generator on fixtures.
#[cfg(test)]
#[path = "../build.rs"]
#[allow(dead_code)]
mod build_script;

#[cfg(test)]
mod tests {
    use super::*;

    fn c(
        login: &'static str,
        display: Option<&'static str>,
        real: Option<&'static str>,
        added: u64,
        deleted: u64,
        first: &'static str,
    ) -> Contributor {
        Contributor {
            login,
            display_name: display,
            real_name: real,
            prs: 0,
            commits: 1,
            lines_added: added,
            lines_deleted: deleted,
            binary_added: 0,
            binary_deleted: 0,
            first_commit: first,
            last_commit: first,
        }
    }

    fn view(names: NameMode, sort: CreditsSort, ascending: bool) -> About {
        About { names, sort, ascending, ..About::default() }
    }

    #[test]
    fn names_fall_back_to_the_username_and_sort_without_case_or_at() {
        let p = c("bob", Some("Bobby"), None, 0, 0, "");
        assert_eq!(
            (p.name(NameMode::Username), p.name(NameMode::DisplayName), p.name(NameMode::RealName)),
            ("@bob".into(), "Bobby".into(), "@bob".into())
        );
        let all = [
            c("zed", None, None, 5, 50, "2026-01-02T00:00:00Z"),
            c("Bob", Some("Ann"), None, 10, 0, "2026-01-01T00:00:00Z"),
            c("alice", None, None, 7, 1, ""),
        ];
        let logins = |v: Vec<&Contributor>| v.into_iter().map(|c| c.login).collect::<Vec<_>>();
        assert_eq!(logins(sorted(&all, &view(NameMode::Username, CreditsSort::Name, true))), ["alice", "Bob", "zed"]);
        assert_eq!(logins(sorted(&all, &view(NameMode::Username, CreditsSort::LinesDelta, false))), ["Bob", "alice", "zed"]);
        assert_eq!(logins(sorted(&all, &view(NameMode::Username, CreditsSort::FirstCommit, true))), ["Bob", "zed", "alice"], "undated last");
    }

    #[test]
    fn numbers_read_with_separators_and_signs() {
        assert_eq!(group(1_234_567), "1,234,567");
        assert_eq!(group(12), "12");
        assert_eq!(
            (count(1, "commit"), count(0, "commit"), count(1200, "contributor")),
            ("1 commit".into(), "0 commits".into(), "1,200 contributors".into())
        );
        assert_eq!(signed(-1000), "−1,000");
        assert_eq!(signed(0), "+0");
        assert_eq!(day("2026-10-07T18:45:23Z"), "2026-10-07");
        assert_eq!(day(""), "—");
        assert_eq!((percent(1, 1000), percent(0, 0), percent(5, 10), percent(20, 10)), ("<1%".into(), "0%".into(), "50%".into(), "100%".into()));
    }

    #[test]
    fn choosing_a_sort_starts_in_its_natural_order_and_again_reverses_it() {
        let mut v = About::default();
        choose_sort(&mut v, CreditsSort::Commits);
        assert_eq!((v.sort, v.ascending), (CreditsSort::Commits, false));
        choose_sort(&mut v, CreditsSort::Commits);
        assert!(v.ascending);
        choose_sort(&mut v, CreditsSort::Name);
        assert_eq!((v.sort, v.ascending), (CreditsSort::Name, true));
        assert_eq!(SORTS.len(), 10);
        assert!(SORTS.iter().all(|s| sort_row(s.0) == *s));
    }

    #[test]
    fn build_rs_takes_names_only_from_verified_entries_and_never_an_email() {
        let toml = r#"
    # A comment, and the template's own lines commented out:
    #   [people.your-github-username]
    [people.alice]
    real_name = "Alice Liddell"   # her own entry
    display_name = 'Al'
    consent_source = "PR #1"

    [people.bob]
    display_name = "bob@example.org"
    real_name = "Robert \"Bob\" Tables"

    [people.carol]
    sync_github_name = true

    [people.dave]
    real_name = "Dave"

    [people.erin]
    real_name = "Erin <erin@mail.example.com>"

    [other]
    what = "x"
    "#;
        let people = super::build_script::parse_people(toml);
        assert_eq!(people.entries.keys().map(String::as_str).collect::<Vec<_>>(), ["alice", "bob", "carol", "dave", "erin"]);
        assert_eq!(people.entries["alice"].display_name.as_deref(), Some("Al"));
        assert_eq!(people.entries["bob"].real_name.as_deref(), Some("Robert \"Bob\" Tables"));
        assert!(people.entries["carol"].sync_github_name);
        assert_eq!(people.problems.iter().map(|p| p.0).collect::<Vec<_>>(), [22, 23], "the unknown table and its key are reported");
        let json = serde_json::json!({
            "schema": 1,
            "commits": 9,
            "contributors": [
                { "login": "alice", "people_entry": true, "github_name": null, "prs": 2, "commits": 4, "lines_added": 10, "lines_deleted": 1, "binary_added": 0, "binary_deleted": 0, "first_commit": "2026-10-01T00:00:00Z", "last_commit": "2026-10-02T00:00:00Z" },
                { "login": "bob", "people_entry": true, "prs": 1, "commits": 1 },
                { "login": "carol", "people_entry": true, "github_name": "Carol from GitHub", "commits": 1 },
                { "login": "dave", "people_entry": false, "commits": 1 },
                { "login": "erin", "people_entry": true, "commits": 1, "first_commit": "erin@mail.example.com" },
                { "login": "not a@login.example.com", "commits": 1 },
                { "login": "frank", "commits": "lots", "prs": -1 },
            ],
            "models": [
                { "company": "Anthropic", "model": "Claude Opus", "version": "5.5", "commits": 8, "lines_added": 9, "lines_deleted": 1 },
                { "company": "noreply@anthropic.com", "model": "X", "version": "1", "commits": 1 },
            ],
        });
        let (code, dropped) = super::build_script::generate(&json, &people.entries);
        let line =
            |login: &str| code.lines().find(|l| l.contains(&format!("login: {login:?}"))).unwrap_or_else(|| panic!("{login}: {code}")).to_string();
        assert!(line("alice").contains(r#"display_name: Some("Al"), real_name: Some("Alice Liddell"), prs: 2, commits: 4"#), "{code}");
        assert!(line("bob").contains(r#"display_name: None, real_name: Some("Robert \"Bob\" Tables")"#), "the email display name is dropped: {code}");
        assert!(line("carol").contains(r#"display_name: Some("Carol from GitHub")"#), "synced from GitHub: {code}");
        assert!(line("dave").contains("display_name: None, real_name: None"), "an unverified entry shows no name: {code}");
        assert!(line("erin").contains("real_name: None") && line("erin").contains(r#"first_commit: """#), "{code}");
        assert!(line("frank").contains("prs: 0, commits: 0"), "bad numbers count as 0: {code}");
        assert!(!code.contains("not a@login"), "an invalid username is skipped: {code}");
        assert!(code.contains("pub static TOTAL_COMMITS: u64 = 9;") && code.contains(r#"company: "", model: "X""#), "{code}");
        assert_eq!(dropped.len(), 4, "{dropped:?}");
        assert!(!code.lines().any(super::build_script::looks_like_email), "{code}");
        // Nothing breaks the build: missing files and garbage give empty tables.
        let (empty, _) = super::build_script::generate(&serde_json::Value::Null, &std::collections::BTreeMap::new());
        assert!(empty.contains("TOTAL_COMMITS: u64 = 0;") && empty.contains("CONTRIBUTORS: &[Contributor] = &[\n];"));
        for odd in
            ["[", "[people.]", "x", "real_name = \"unclosed", "[people.a]\nreal_name = \"\\u12\"", "= =", "[people.a]\nsync_github_name = maybe"]
        {
            let _ = super::build_script::parse_people(odd);
        }
        for (s, email) in [("a@b.co", true), ("<x@y.org>", true), ("@pkkid", false), ("Bob @ home", false), ("x@localhost", false), ("Ann", false)] {
            assert_eq!(super::build_script::looks_like_email(s), email, "{s}");
        }
    }

    #[test]
    fn no_email_reaches_the_binary() {
        let b = super::Baked::compiled();
        let mut strings: Vec<&str> = vec![super::compiled::SOURCE];
        for c in b.contributors {
            strings.extend([c.login, c.first_commit, c.last_commit]);
            strings.extend(c.display_name);
            strings.extend(c.real_name);
            assert!(super::build_script::valid_login(c.login), "{}", c.login);
        }
        for m in b.models {
            strings.extend([m.company, m.model, m.version]);
        }
        for s in strings {
            assert!(!super::build_script::looks_like_email(s) && !s.contains("noreply"), "{s}");
        }
        // The compiled tables are the committed file's, at least its contributors and models.
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contributors/contributors.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(b.contributors.len(), json["contributors"].as_array().unwrap().len());
        assert_eq!(b.models.len(), json["models"].as_array().unwrap().len());
        assert_eq!(b.total_commits, json["commits"].as_u64().unwrap());
    }
}
