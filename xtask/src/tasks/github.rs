//! What the task board needs from GitHub, as a trait: the `gh` CLI implements it (`gh.rs`), and an
//! in-memory fake implements it for tests (`fake.rs`).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Todo,
    Ready,
    InProgress,
    InReview,
    Blocked,
    Done,
}

impl Status {
    pub const ALL: [Status; 6] = [Status::Todo, Status::Ready, Status::InProgress, Status::InReview, Status::Blocked, Status::Done];

    pub fn name(self) -> &'static str {
        match self {
            Status::Todo => "Todo",
            Status::Ready => "Ready",
            Status::InProgress => "In Progress",
            Status::InReview => "In Review",
            Status::Blocked => "Blocked",
            Status::Done => "Done",
        }
    }

    pub fn from_name(name: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|s| s.name() == name)
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextField {
    Agent,
    Branch,
}

impl TextField {
    pub fn name(self) -> &'static str {
        match self {
            TextField::Agent => "Agent",
            TextField::Branch => "Branch",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub open: bool,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    /// Milestone title.
    pub milestone: Option<String>,
}

impl Issue {
    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }
}

/// A project item (one per issue), in board order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub issue: u64,
    pub status: Option<Status>,
    pub agent: String,
    pub branch: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewIssue {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub milestone: Option<String>,
    pub assignee: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueEdit {
    pub title: Option<String>,
    pub body: Option<String>,
    pub add_labels: Vec<String>,
    pub remove_labels: Vec<String>,
    pub milestone: Option<String>,
    pub add_assignee: Option<String>,
}

impl IssueEdit {
    pub fn is_empty(&self) -> bool {
        *self == IssueEdit::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneInfo {
    /// GitHub's number for the milestone (not the roadmap's `M<n>`).
    pub number: u64,
    pub title: String,
    pub description: String,
    pub open: bool,
    /// Issues in the milestone, open and closed.
    pub issues: u64,
}

/// Whether GitHub can merge a pull request, as its `mergeable` field says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeState {
    Mergeable,
    Conflicting,
    /// GitHub has not (re)computed it yet, e.g. right after a push; ask again shortly.
    Unknown,
}

impl MergeState {
    /// `MERGEABLE`, `CONFLICTING`; anything else (`UNKNOWN`) is [`MergeState::Unknown`].
    pub fn from_graphql(value: &str) -> MergeState {
        match value {
            "MERGEABLE" => MergeState::Mergeable,
            "CONFLICTING" => MergeState::Conflicting,
            _ => MergeState::Unknown,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectFields {
    pub status_options: Vec<String>,
    pub text_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    /// Label names on the pull request.
    pub labels: Vec<String>,
}

/// GitHub's error text when [`GitHub::add_item`] names an issue already on the project (the
/// project's auto-add workflow adds new issues itself).
pub const ALREADY_IN_PROJECT: &str = "Content already exists in this project";

/// GitHub's error text when [`GitHub::squash_merge`] comes before it has recomputed a pull
/// request's mergeability after a push (`GraphQL: Pull Request is not mergeable (mergePullRequest)`).
pub const NOT_MERGEABLE: &str = "Pull Request is not mergeable";

/// Every call is a single GitHub read or write. Errors are human-readable strings.
pub trait GitHub: Send + Sync {
    /// Every issue of the repository, open and closed.
    fn issues(&self) -> Result<Vec<Issue>, String>;
    fn create_issue(&self, issue: &NewIssue) -> Result<u64, String>;
    fn edit_issue(&self, number: u64, edit: &IssueEdit) -> Result<(), String>;
    fn comment(&self, number: u64, body: &str) -> Result<(), String>;
    fn close_issue(&self, number: u64, comment: &str) -> Result<(), String>;

    /// The project's items in board order.
    fn items(&self) -> Result<Vec<Item>, String>;
    /// Adds an issue to the project and returns the new item id.
    fn add_item(&self, issue: u64) -> Result<String, String>;
    fn set_status(&self, item: &str, status: Status) -> Result<(), String>;
    /// Sets a text field; an empty value clears it.
    fn set_field(&self, item: &str, field: TextField, value: &str) -> Result<(), String>;
    /// Moves an item right after `after` (`None` = to the top).
    fn move_item(&self, item: &str, after: Option<&str>) -> Result<(), String>;

    fn milestones(&self) -> Result<Vec<MilestoneInfo>, String>;
    fn create_milestone(&self, title: &str, description: &str) -> Result<(), String>;
    /// Sets a milestone's title and description (`number` is GitHub's milestone number).
    fn edit_milestone(&self, number: u64, title: &str, description: &str) -> Result<(), String>;
    fn close_milestone(&self, number: u64) -> Result<(), String>;
    fn labels(&self) -> Result<Vec<String>, String>;
    fn create_label(&self, name: &str, color: &str, description: &str) -> Result<(), String>;
    fn project_fields(&self) -> Result<ProjectFields, String>;
    fn create_text_field(&self, name: &str) -> Result<(), String>;

    fn find_pr(&self, branch: &str) -> Result<Option<PullRequest>, String>;
    fn open_pr(&self, branch: &str, title: &str, body: &str) -> Result<PullRequest, String>;
    /// The latest finished CI run on `main` when it failed (its title and URL), else `None`.
    fn main_ci_failure(&self) -> Result<Option<String>, String>;
    /// Whether GitHub can merge pull request `pr` with `head` as its head commit; a different head
    /// (GitHub has not seen the push yet) is [`MergeState::Unknown`].
    fn pr_mergeable(&self, pr: u64, head: &str) -> Result<MergeState, String>;
    fn squash_merge(&self, pr: u64, subject: &str) -> Result<(), String>;
}
