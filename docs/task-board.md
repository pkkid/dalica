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
| **The plan:** every task, its dependencies, files, scope and *done when* | task blocks in [`roadmap.md`](roadmap.md), versioned with the code | the lead and maintainer, at milestone reviews |
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
  top to bottom in build order. Review gates (`M1.A`, `M1.R`) are issues too, assigned to the maintainer.
- **Labels:** `task` (every task issue), `gate` (review gates), `needs-maintainer` (an agent is
  blocked on a question), `stale` (claimed but idle, set by `status`).
- **Project fields** (project 3, "Dalica"):
  - **Status:** `Todo` (a dependency is not done) · `Ready` (all dependencies done; free to claim) ·
    `In Progress` (claimed) · `In Review` (pull request open) · `Blocked` (needs the maintainer) ·
    `Done` (merged).
  - **Agent** (text): the claiming agent's name, e.g. `agent-3`.
  - **Branch** (text): e.g. `task/M1.4-scenes`.
  - Built-in fields stay: Milestone, Linked pull requests, Labels.

`cargo xtask tasks setup` creates the milestones, labels, the extra Status options and the Agent
and Branch fields. It is idempotent, and the maintainer approves its first run (it writes to the
public repository and the project).

## 3. Task block format (in `roadmap.md`)

```markdown
- ⬜ **M1.4 `scenes`**
  - after: M1.1
  - owner: agent                     (optional; `maintainer` for review gates)
  - files: `crates/scenes/**`
  - scope: what to build, in a few lines.
  - done when: the command or test that proves it, with its edge cases.
```

- **Id:** `M<milestone>.<n>` for work, `M<n>.R` for the milestone review gate, `M<n>.A`, `M<n>.B`…
  for checkpoint gates. Ids are never reused or renumbered.
- **Checkbox:** ⬜ or ✅ only. Live progress is on the board, not in this file.
- **after:** comma-separated ids, or `—`. A milestone's first tasks list the previous milestone's
  review gate, so nothing starts before the maintainer has reviewed.
- **Order:** the order of blocks in `roadmap.md` is the order work is picked up, and it must be a
  valid build order: every id in `after` appears earlier in the file (the parser fails
  otherwise). Ids are names, not positions: M0.10 sits right after M0.2 and is picked up before
  M0.3. To reprioritise, move blocks in `roadmap.md` (the lead, at a review); never renumber.
- **files:** comma-separated backticked paths or globs the task may create or change (shared docs
  such as `roadmap.md`, `parity.md` and `devlog` entries are exempt). Two tasks whose `files`
  overlap are never in progress at the same time. If a task turns out to need a file outside its
  list, the agent asks the lead (comment on the issue) before touching it.
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
| gate → Done | the maintainer closes the gate issue after the review; the lead ticks ✅ | maintainer, lead |

## 5. Commands (`cargo xtask tasks …`)

| Command | What it does |
|---|---|
| `setup [--dry-run]` | create milestones, labels and the Agent/Branch fields (idempotent; maintainer approves). Missing Status options are reported, not created: replacing a single-select field's options through the API resets every item |
| `sync [--check\|--dry-run]` | parse `roadmap.md`, create or update issues and project items, move Todo ↔ Ready, keep the board in roadmap order, close the issue of a ✅ task once its `M<n>.<k>:` commit is on `main` and nothing that isn't merged; `--check` only parses (offline, used by `ci`); `--dry-run` reads GitHub and prints the writes it would make |
| `next [--agent <name>]` | list Ready tasks this agent may take: dependencies done, files not overlapping any In Progress or In Review task, milestone not behind an open gate. Prints them in `roadmap.md` order, so the first line is the one to claim |
| `claim <id> --agent <name>` | under the claim lock, re-read the board, refuse if no longer Ready or overlapping, then set In Progress + Agent + Branch, comment "claimed by agent-3 on task/M1.4-scenes", create or reuse the agent's worktree and branch from `origin/main`, and print the agent's environment |
| `env <name>` | print the agent's environment (`eval "$(cargo xtask tasks env agent-3)"`) |
| `submit <id> [--body-file F]` | refuse unless `cargo xtask ci` passed on this commit (a green `ci` on a clean worktree records `<git common dir>/dalica-ci/<commit>.ok`) and a devlog entry exists; push the branch; open the PR (title `M1.4: <title>`, body: what changed, evidence for each *done when*, numbers, screenshots, `Closes #<issue>`); set In Review |
| `release <id> --reason …` | back to Ready with a comment; the branch is kept for whoever claims it next |
| `block <id> "question"` | set Blocked, add `needs-maintainer`, comment the question; the agent then takes another task |
| `merge <id>` | lead only, under the merge lock: rebase the branch on `origin/main`, `cargo xtask ci`, push, wait for GitHub CI, squash-merge (`M1.4: <title> (#PR)`), then `sync` |
| `status` | the whole board: per milestone, per status, who holds what, stale claims (no push or log entry for 3 h) |
| `log <id> "text"` | append a dated entry to the private dev log (§8) |

`xtask` drives GitHub through the `gh` CLI (already authenticated on the maintainer's machine). This
is developer tooling; the Dalica app itself never touches the network for this.

## 6. Roles

- **Maintainer:** runs `tasks setup` once, reviews each gate (closes `M<n>.A` / `M<n>.R`), answers
  `needs-maintainer` questions, decides reserved decisions. Watches the project board.
- **Lead (`agent-1`):** the integrator. Loop: `tasks status` → merge In Review tasks one at a time
  (`tasks merge`) → copy reported metrics into `docs/status.md` → `tasks sync` → release stale
  claims → when a milestone's last task merges, run `cargo xtask accept M<n>`, write *Where we
  stand*, draft the next milestone's task blocks, and hand the gate to the maintainer. The lead
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
  runs are memory-bound: `cargo xtask ci` lowers test parallelism when free RAM is low.
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

Every task gets one entry before `submit` (what landed, what is still open, numbers). The lead
adds one per merge and per milestone review. Committed history lives in git; the dev log is the
private narrative the next agent reads after a crash.

## 9. Tests (M0.10)

The `GitHub` trait has an in-memory fake; tests cover parsing every block of `roadmap.md`, `sync`
idempotence, `next` (dependencies, file overlap with globs, gates), 8 concurrent claimers on one
task (exactly one wins), stale detection on a virtual clock, and `sync` failing when `roadmap.md`,
the issues and `main` disagree.
