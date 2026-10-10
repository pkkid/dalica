//! Where the board lives and the fixed names it uses (`docs/task-board.md` §2).

pub const REPO: &str = "pkkid/dalica";
pub const OWNER: &str = "pkkid";
pub const PROJECT: u32 = 3;
/// Review gates are assigned to the maintainer.
pub const MAINTAINER: &str = "pkkid";
/// The lead integrator: only it may `merge`.
pub const LEAD: &str = "agent-1";
pub const BLOB_URL: &str = "https://github.com/pkkid/dalica/blob/main";

/// Markers around the generated part of an issue body.
pub const START: &str = "<!-- dalica:task -->";
pub const END: &str = "<!-- /dalica:task -->";

pub const LABEL_TASK: &str = "task";
pub const LABEL_GATE: &str = "gate";
pub const LABEL_BLOCKED: &str = "needs-maintainer";
pub const LABEL_STALE: &str = "stale";
/// (name, colour, description) created by `setup`.
pub const LABELS: [(&str, &str, &str); 4] = [
    (LABEL_TASK, "1d76db", "A roadmap task (docs/task-board.md)"),
    (LABEL_GATE, "5319e7", "Review gate owned by the maintainer"),
    (LABEL_BLOCKED, "d93f0b", "An agent is blocked on a question for the maintainer"),
    (LABEL_STALE, "fbca04", "Claimed but idle; the lead may release it"),
];

/// A claim with no push and no dev-log entry for this long is stale.
pub const STALE_SECS: i64 = 3 * 60 * 60;

/// Shared docs and registration files every task may touch; they never count as overlapping
/// (`task-board.md` §3).
pub const SHARED_FILES: [&str; 6] =
    ["docs/roadmap.md", "docs/parity.md", "Cargo.toml", "Cargo.lock", "xtask/src/main.rs", "tests/acceptance/Cargo.toml"];
