# Task board

How agents (and the maintainer) know which tasks exist, which are free, who is working on what,
and what is done. Read this with [`roadmap.md`](roadmap.md) (the plan) and `AGENTS.md` §7 (the
rules for parallel agents).

> Status: built (M0.10). On 2026-10-09 a one-off bootstrap created the labels, the 38 milestones,
> the project's Status options and Agent/Branch fields, and issues #1–#32 for the M0 and M1 task
> blocks; the first `cargo xtask tasks sync` the same day adopted them (matched by task id in the
> title) with no duplicate: its `--dry-run` against the bootstrap showed zero writes. Until parallel
> workers start (M2), the single agent sets In Progress by hand and lets `sync` mark merged tasks
> Done (`AGENTS.md` §4).

## 1. Three places, three jobs

| What | Where | Changed by |
|---|---|---|
| **The plan:** every task, its dependencies, files, scope and *done when* | task blocks in [`roadmap.md`](roadmap.md), versioned with the code | the lead, ahead of each milestone (no approval needed) |
| **The live state:** ready, claimed (by whom, on which branch), in review, blocked | GitHub: one issue per task in [pkkid/dalica](https://github.com/pkkid/dalica/issues), all on the project board [Dalica](https://github.com/users/pkkid/projects/3) | agents, only through `cargo xtask tasks` |
| **Done** | the task's squash commit (`M1.4: …`) is on `main` and its block is ticked ✅ in `roadmap.md` | the worker ticks ✅ in its branch; it becomes true when the lead merges |

Nothing else counts: a ✅ on an unmerged branch, a closed issue without a merge, or a comment saying
"done" is not done. `cargo xtask tasks sync` fails when the three disagree.

## 2. GitHub objects

- **Milestones** (repository milestones): one per roadmap milestone, titled `M1 Star a photo, end
  to end`. The project's built-in *Milestone* field shows them.
- **Issues:** one per task block, titled `M1.4 scenes` (id + the block's title). The body holds a
  generated section between `<!-- dalica:task -->` markers (after, owner, files, scope, done when,
  a link to the roadmap block); `sync` rewrites only that section, so humans may add notes below
  it. `sync` also keeps the project's item order equal to `roadmap.md` order, so the board reads
  top to bottom in build order. Review tasks (`M2.R`, `M3.R`, `M6.R`) are issues too, assigned to the
  maintainer.
- **Labels:** `task` (every task issue), `gate` (review tasks), `needs-maintainer` (an agent is
  blocked on a question), `stale` (claimed but idle, set by `status`), and `baseline-approved`
  (pull requests; required by nothing since fingerprint baselines are approved in bulk at reviews,
  §7; `setup` still creates it so a per-PR gate could return, and agents never set it).
- **Project fields** (project 3, "Dalica"):
  - **Status:** `Todo` (a dependency is not done) · `Ready` (all dependencies done; free to claim) ·
    `In Progress` (claimed) · `In Review` (pull request open) · `Blocked` (needs the maintainer) ·
    `Done` (merged).
  - **Agent** (text): the claiming agent's name, e.g. `agent-3`.
  - **Branch** (text): e.g. `task/M1.4-scenes`.
  - Built-in fields stay: Milestone, Linked pull requests, Labels.

`cargo xtask tasks setup` creates the milestones, the labels (including the unused `baseline-approved`) and the
Agent and Branch fields, and keeps the milestones' titles and descriptions equal to `roadmap.md` (§5).
It is idempotent, and the maintainer approves its first run (it writes to the public repository and
the project).

## 3. Task block format (in `roadmap.md`)

```markdown
- ⬜ **M1.4 `scenes`**
  - after: M1.1
  - owner: agent                     (optional; `lead` for board upkeep and planning, `maintainer` for every review task)
  - files: `crates/scenes/**`
  - scope: what to build, in a few lines.
  - done when: the command or test that proves it, with its edge cases.
```

- **Id:** `M<milestone>.<n>` for work, `M<n>.R` for the milestone's one review task, at its end (a
  release gate in M6, M10 and M15). There are no mid-milestone reviews (maintainer rule,
  2026-10-10: "I do not want any checkins mid milestones"): the parser refuses an open `M<n>.A`,
  `M<n>.B`, …; the finished `M1.A` stays as history. Ids are never reused or
  renumbered (the milestones were renumbered once, on 2026-10-10, before any task past M3
  existed).
- **owner:** `agent` (default, any worker), `lead` (only `agent-1` may claim it: board upkeep,
  planning), `maintainer` (no agent claims it). Every review task is `maintainer` (maintainer rule,
  2026-10-10; the parser refuses a review task with any other owner).
- **Review tasks are stops.** An open review task holds back every task listed after it in
  `roadmap.md`, in its own milestone and later ones, until the maintainer closes its issue. The
  lead prepares it (`/milestone-review`: the check-in page and a pull request with *Where we
  stand* and the `status.md` numbers); only the maintainer merges that pull request and closes the
  issue. `tasks submit` and `tasks merge` refuse a review task, and `tasks sync` never closes a
  review's issue.
- **Checkbox:** ⬜ or ✅ only. Live progress is on the board, not in this file.
- **after:** comma-separated ids, or `—`. A milestone's first tasks list the work they build on; a
  review listed before them already holds them back by its place in the file, so `after` names a
  review only to make the dependency visible (after a release, the release gate).
- **Order:** the order of blocks in `roadmap.md` is the order work is picked up, and it must be a
  valid build order: every id in `after` appears earlier in the file (the parser fails
  otherwise). Ids are names, not positions: M0.10 sits right after M0.2 and is picked up before
  M0.3. To reprioritise, move blocks in `roadmap.md` (the lead); never renumber.
- **files:** comma-separated backticked paths or globs the task may create or change (shared docs
  such as `roadmap.md`, `parity.md` and `devlog` entries are exempt). Two tasks whose `files`
  overlap are never in progress at the same time. If a task turns out to need a file outside its
  list, the agent asks the lead (comment on the issue) before touching it.
- **Shared registration files** need not be listed and never count as overlap: the root
  `Cargo.toml` (new `[workspace.dependencies]` lines), `Cargo.lock`, `xtask/src/main.rs` (one
  dispatch line per new command) and `tests/acceptance/Cargo.toml` (one `[[test]]` per milestone
  file); `SHARED_FILES` in `xtask/src/tasks/config.rs` is the list the board uses. A task may also
  make the matching registration edits in a crate it owns files in (its `Cargo.toml` dependencies,
  the `mod` lines of its `lib.rs`) without listing them. Edits there are small and surgical
  (re-read the file right before editing); the lead resolves conflicts in them at merge,
  re-resolving `Cargo.lock` rather than hand-merging it.
- **Small primitives** (M1.A decision): when a task needs one small, tested addition to a lower crate
  it depends on (a function, a field, an enum variant) and the alternative is a workaround, it may
  make it outside its `files`, says so on its issue and in the commit body, and keeps the edit
  additive. The lead checks at merge that no two tasks in review touched the same file this way.
- **done when** never writes `docs/status.md` (metrics go in the dev log and the pull request; the
  lead copies them) and never needs the maintainer mid-task (approvals, pushes and manual looks
  belong to review tasks, their check-in pages or the lead).
- The parser fails with a line number on a malformed block; `cargo xtask ci` runs it.

## 4. Lifecycle

```text
Todo ──(deps done)──► Ready ──claim──► In Progress ──submit──► In Review ──lead merges──► Done
                        ▲                 │    │                   │
                        └──── release ────┘    └── block ──► Blocked (maintainer answers → Ready)
                        └──────────── lead sends back for a bigger fix ┘
```

| Transition | Command | Who |
|---|---|---|
| Todo → Ready | `tasks sync` (when every `after` id is ✅ on `main`) | anyone; the lead runs it after each merge |
| Ready → In Progress | `tasks claim <id> --agent <name>` | a worker (or the lead) |
| In Progress → In Review | `tasks submit <id>` | the claiming worker |
| In Review → Done | `tasks merge <id>` | the lead only |
| In Progress → Ready | `tasks release <id> --reason "…"` | the claiming worker, or the lead for stale claims |
| In Progress → Blocked | `tasks block <id> "question"` | the claiming worker |
| Blocked → Ready | the maintainer answers on the issue and removes `needs-maintainer`; `sync` | the maintainer |
| review task: prepared | `/milestone-review`: the check-in page, a `review/M<n>.R` pull request (✅ ticked, *Where we stand*, `status.md`) linked on the issue; the run stops | lead |
| review task → Done | the maintainer merges the review's pull request and closes its issue; `sync` (everything listed after it can then be claimed) | maintainer only |

## 5. Commands (`cargo xtask tasks …`)

| Command | What it does |
|---|---|
| `setup [--dry-run]` | create milestones, labels (`task`, `gate`, `needs-maintainer`, `stale`, `baseline-approved`) and the Agent/Branch fields (idempotent; maintainer approves). A GitHub milestone matches a roadmap milestone by its exact title, else by its `M<n>` number, and is then renamed (and its description set to the milestone's goal) to match `roadmap.md`; the roadmap renamed its milestones once. A GitHub milestone with no roadmap counterpart is closed when it has no issues and otherwise reported, never deleted. Missing Status options are reported, not created: replacing a single-select field's options through the API resets every item |
| `sync [--check\|--dry-run]` | parse `roadmap.md`, create or update issues and project items, move Todo ↔ Ready, keep the board in roadmap order, close the issue of a ✅ task once its `M<n>.<k>:` commit is on `main` and nothing that isn't merged; never closes a review's issue (the maintainer does); `--check` only parses (offline, used by `ci`); `--dry-run` reads GitHub and prints the writes it would make. Before any write it checks that the milestones it would put issues in exist, and otherwise says to run `tasks setup` |
| `next [--agent <name>]` | list Ready tasks this agent may take: dependencies done, files not overlapping any In Progress or In Review task, not listed after an open review task, `owner: lead` tasks only for the lead. Prints them in `roadmap.md` order, so the first line is the one to claim |
| `claim <id> --agent <name>` | under the claim lock, re-read the board, refuse if no longer Ready or overlapping, then set In Progress + Agent + Branch, comment "claimed by agent-3 on task/M1.4-scenes", create or reuse the agent's worktree and branch from `origin/main`, and print the agent's environment |
| `env <name>` | print the agent's environment (`eval "$(cargo xtask tasks env agent-3)"`) |
| `submit <id> [--body-file F] [--dry-run]` | refuse a review task; refuse unless `cargo xtask ci` passed on this commit (a green `ci` on a clean worktree records `<git common dir>/dalica-ci/<commit>.ok`) and a devlog entry exists; push the branch; open the PR (title `M1.4: <title>`, body: what changed, evidence for each *done when*, numbers, screenshots, `Closes #<issue>`); set In Review. The evidence is the `--body-file` (screenshots, per-point evidence) or, without it, the task's newest dev-log entry. When the branch adds, changes or deletes **fingerprint baselines** (`**/fingerprints/*.json`), the body lists them (each marked added, changed or deleted) and says they are approved in bulk at the next review; the worker attaches their images (the new PNG, or the actual/expected/diff PNGs) and the commit body says why each changed |
| `release <id> --reason …` | back to Ready with a comment; the branch is kept for whoever claims it next |
| `block <id> "question"` | set Blocked, add `needs-maintainer`, comment the question; the agent then takes another task |
| `merge <id> [--dry-run]` | lead only, under the merge lock: refuse a review task (only the maintainer merges those), refuse while the latest finished GitHub CI run on `main` is red, rebase origin's copy of the branch on `origin/main` in the `merge` worktree (the worker may still have the branch checked out), `cargo xtask ci`, push, wait until GitHub reports the pushed head of the PR mergeable (it recomputes that after a push and refuses a squash before: read at once, then with backoff for up to 2 minutes; a conflict refuses at once, and nothing is merged either way), squash-merge (`M1.4: <title> (#PR)`, retried up to three times while GitHub still answers "not mergeable") without waiting for the PR's GitHub checks (they and the run on `main` still report); when the branch adds, changes or deletes fingerprint baselines, append a dev-log entry for the next review with one `FINGERPRINTS: <id> #<PR> added\|changed\|deleted <paths>` line per kind (from the local `git diff`, no extra GitHub call; only after the squash succeeded); then `sync` against `docs/roadmap.md` as merged on `origin/main` (fetched and read from git, so the task's ✅ counts before the main checkout is pulled; no checkout, clean or dirty, is touched) |
| `status` | the whole board: per milestone, per status, who holds what, stale claims (no push or log entry for 3 h) |
| `log <id> "text"` | append a dated entry to the private dev log (§8) |
| `prune [--dry-run]` | lead only, at each review (or when disk runs low): remove cargo's `incremental/` caches from `target/agent-<n>` and `target/merge` (§7). Skips an agent that holds an In Progress task, and a profile where cargo is building (it takes cargo's own build locks first, so a cargo that starts meanwhile waits). Only an `incremental/` next to a `.fingerprint/` is removed; symlinks are never followed. Prints what it removed and the space freed |

`--dry-run` (with `setup`, `sync`, `submit`, `merge` and `prune`) reads GitHub and git, prints every
write it would make (pushes, rebases, `ci` runs, the PR body, board changes) and what `prune` would
remove, and makes none: it takes no lock, deletes nothing and writes nothing to GitHub.

`xtask` drives GitHub through the `gh` CLI (already authenticated on the maintainer's machine). This
is developer tooling; the Dalica app itself never touches the network for this.

## 6. Roles

- **Maintainer:** runs `tasks setup` once; decides every review task: reads its check-in page,
  merges its pull request and closes its issue (feedback becomes tasks), and signs off the releases
  (M6, M10, M15) the same way; then starts the next run (`/milestone-start`). Answers
  `needs-maintainer` questions (only the kinds `AGENTS.md` §4 lists), decides reserved decisions.
- **Lead (`agent-1`):** the integrator. Loop: `tasks status` → merge In Review tasks one at a time
  (`tasks merge`, re-resolving `Cargo.lock` if two branches both changed it) → copy reported
  metrics into `docs/status.md` → `tasks sync` → release stale
  claims → write the next milestone's task blocks while the current one runs. When the run reaches
  a review task, let running tasks finish and merge, prepare it (`/milestone-review`: `accept M<n>`,
  the check-in page, *Where we stand* and `status.md` on a `review/M<n>.R` branch, a pull
  request) and stop; never merge that pull request or close the review issue. When GitHub CI on `main` turns red, fix it
  before the next merge (`tasks merge` refuses until it is green). The lead
  may take small tasks when the review queue is empty, never ones whose files overlap a task in
  review.
- **Workers (`agent-2` … `agent-8`):** loop: `tasks next` → `tasks claim` → do the task in its
  worktree following `AGENTS.md` → `cargo xtask ci` → `tasks log` → `tasks submit` → next. When
  `next` offers nothing, wait 5 minutes and ask again (do not poll faster; GitHub rate limits are
  shared by all agents).

## 7. Concurrency on one machine

All agents run on the maintainer's machine, so local locks make GitHub updates race-free:

- **Claim lock:** `<git common dir>/dalica-tasks.lock` (the common `.git` directory is shared by
  every worktree). `claim`, `release`, `block` and `submit` hold it while they re-read the board
  from GitHub and write it back, so two agents can never claim the same task or overlapping files.
- **Merge lock:** `<git common dir>/dalica-merge.lock`, held by `tasks merge`, so merges into
  `main` happen one at a time (two individually green branches can still break each other).
- **Per-agent resources** (printed by `tasks env`):

  | Resource | Value for `agent-<n>` |
  |---|---|
  | Worktree | `../dalica.worktrees/agent-<n>` (reused from task to task; a new branch each task) |
  | Cargo target dir | `<main checkout>/target/agent-<n>` (kept warm across tasks; `[profile.dev] debug = "line-tables-only"` keeps it small) |
  | Build jobs | `CARGO_BUILD_JOBS=3` (24 cores shared by 8 agents) |
  | Control port | `18000 + 10·n` (never the default 7990) |
  | Scratch dir | `<scratchpad>/agent-<n>/` |

  On this machine (24 cores, 31 GB RAM, ~134 GB free on 2026-10-09) eight agents fit, but test
  runs are memory-bound: `cargo xtask ci` lowers test parallelism when free RAM is low. Disk is
  the other limit: a long-lived target dir grows from ~3.5 GB to ~10 GB, mostly `incremental/`, so
  the lead runs `cargo xtask tasks prune` at each review.
- **Fingerprint approvals:** fingerprint baselines are approved **in bulk at the next review**
  (the milestone's `M<n>.R`), not on each pull request (the maintainer's decision, 2026-10-10: in M3
  nearly every task touches fingerprints, and a per-PR approval stopped the build on each). A
  branch that adds, changes or deletes a baseline still shows what changed on its PR (the new PNG,
  or the diff images) and says why in the commit body (`AGENTS.md` §3); `merge` merges it without
  waiting and records the baselines in the dev log as `FINGERPRINTS:` lines (§5, §8). The review
  page lists every baseline recorded since the last review with its PR and images; the maintainer
  approves them all or names the ones to redo, and each rejected baseline becomes a fix task.
- **Shared files:** workers may tick their own task's ✅ and update their own parity rows (different
  lines, so rebases stay clean). Workers never edit `docs/status.md` (the lead owns it), never edit
  another task's block, and never fix files owned by another in-progress task: they comment on
  that task's issue instead.

## 8. The dev log (private)

`log/devlog.md` in the **main checkout** (gitignored, never committed; `log/` in a worktree is not
used). `cargo xtask tasks log M1.4 "…"` appends, under the claim lock:

```markdown
## 2026-10-09 14:02 · agent-3 · M1.4 scenes
Landed: seeded gradients/horizons/blobs, JPEG + EXIF (orientations 1–8), PNG, demo_library.
Open: embedded EXIF thumbnails are fixed 160×120; M1.5 may want other sizes.
Numbers: 200-photo demo library in 1.9 s.
```

Every task gets one entry before `submit` (what landed, what is still open, numbers); without
`--body-file`, `submit` uses the task's newest entry as the PR's evidence. The lead
adds one per merge and per review. `merge` itself appends an entry, under the lead's name, for a
branch that adds, changes or deletes fingerprint baselines:

```markdown
## 2026-10-12 09:40 · agent-1 · M3.4 Icons
FINGERPRINTS: M3.4 #120 added crates/ui-kit/tests/fingerprints/icons-light.json crates/ui-kit/tests/fingerprints/icons-dark.json
FINGERPRINTS: M3.4 #120 changed crates/ui/tests/fingerprints/window-1400.json
```

The review (`/milestone-review`) lists every `FINGERPRINTS:` line written since the previous review
for the maintainer to approve (§7).

Committed history lives in git; the dev log is the private narrative the next agent reads after a
crash.

## 9. Tests (M0.10)

The `GitHub` trait has an in-memory fake; tests cover parsing every block of `roadmap.md`, `sync`
idempotence, `next` (dependencies, file overlap with globs, gates), 8 concurrent claimers on one
task (exactly one wins), stale detection on a virtual clock, and `sync` failing when `roadmap.md`,
the issues and `main` disagree. Since M2.7 they also cover `submit`'s dev-log default and its
baseline list, `submit`/`merge` dry runs that write nothing,
and `prune` on temporary target dirs (idle and busy agents, a held cargo lock, symlinks, dry run).
Since the review-stop rule (2026-10-10) they also cover an open review holding back every task
listed after it (in its milestone and later ones) until its issue is closed, `submit` and `merge`
refusing review tasks, and `sync` never closing a review's issue. Since the M2 run (2026-10-10) they
also cover `merge` waiting for GitHub's mergeability (unknown then mergeable, a conflict, a bounded
give-up, "not mergeable" squash retries), the post-merge `sync` reading `origin/main`'s roadmap
without touching a dirty checkout, and `setup` renaming, describing and closing milestones (and its
dry run). Since bulk baseline approval (2026-10-10) they cover `merge` merging a branch with
fingerprint changes without any label and recording one `FINGERPRINTS:` line per kind (added,
changed, deleted), recording nothing for a branch without fingerprints or for a merge that failed,
its dry run reporting the dev-log write instead of making it, and `changed_paths` telling adds,
changes and deletes apart on a real repository.
