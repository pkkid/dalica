---
name: milestone-start
description: Run the build from a milestone (M2 and later) with this session as the lead (agent-1) and parallel worker subagents, one per task, until the next review task (any M<n>.A or M<n>.R); there it prepares the review with /milestone-review and stops for the maintainer, who merges the review PR, closes the issue and runs this again. Run only when the maintainer asks.
argument-hint: "[M<n>] [workers, default 7]"
disable-model-invocation: true
---

# Run the milestones

Arguments: **$ARGUMENTS**. The first argument is the milestone (`M<n>`). If it is missing, use the
first milestone with Ready tasks in `cargo xtask tasks status`. The second argument is the number of
workers. The default and maximum is **7**, so the lead plus 7 workers makes the 8 agents the machine
is sized for (`AGENTS.md` §7, `docs/task-board.md` §7).

This session is the **lead, `agent-1`** (`docs/task-board.md` §6). It never writes product code.
Its jobs are to claim, dispatch, review, merge and keep the board true.

- **Workers** are subagents named `agent-2` … `agent-<workers+1>`. Each subagent does **one task**
  and returns.
- A slot (`agent-<k>`) keeps its worktree, target dir, port and scratch dir from task to task, so
  builds stay warm.
- **Run until the next review task** (any `M<n>.A` or `M<n>.R`, release gates included;
  `docs/roadmap.md` → How milestones work). Every review task is a stop: the board offers nothing
  listed after it until the maintainer closes it. At a review the lead prepares it with
  `/milestone-review` and **stops**. Only the maintainer merges the review's pull request and
  closes its issue; the lead never does either.
- **Decide, don't ask.** Technical decisions are the agents' (`AGENTS.md` §4). Only the short list
  there reaches the maintainer. The lead's own choices (board, process, scheduling) follow "The
  lead asks even less" there: decide, and log each one as a `DEFERRED:` dev-log line for the
  check-in.

## 1. Preflight (stop and report if any check fails)

1. Read `AGENTS.md`, `docs/task-board.md`, `docs/status.md`, the milestone's section in
   `docs/roadmap.md` (Goal, In, Out, Done when, task blocks), and the last review entry in
   `log/devlog.md`.
2. **Is the main checkout ready?** It must be on `main`, clean, and equal to `origin/main` after
   `git fetch`. `cargo xtask ci` must be green there (use your own target dir,
   `target/agent-1`).
3. **Is the board ready?**
   - The milestone's task blocks exist on `main`.
   - `cargo xtask tasks sync` has created their issues.
   - Its first tasks show Ready in `cargo xtask tasks status`, and `cargo xtask tasks next`
     offers them: no review task listed before them is still open. If one is (its PR not merged
     or its issue not closed), stop and tell the maintainer which review waits for them.
4. **Are there leftovers?** Nothing should be In Progress from an earlier run. For each worktree in
   `git worktree list`, check `git status` and `git log origin/main..HEAD`. Finish, release
   (`tasks release <id> --reason …`) or report it before claiming anything.
5. **Are resources available?**
   - `gh auth status` is OK.
   - Free disk ≥ 60 GB (each agent target dir grows to about 5 GB, plus acceptance copies).
   - Free RAM: say how much.
6. **Plan the parallelism.**
   - List the milestone's tasks in roadmap order with `after` and `files`.
   - Work out the waves and how many tasks can run at once given `files` overlap.
   - Find the next review task: the run stops there.
   - If fewer tasks can run at once than there are workers, use fewer workers and say so.
7. Show the maintainer a short plan: milestone, worker count, waves, the review task where the
   run will stop, and any preflight warning. Then start; the command itself is the go-ahead. The maintainer's
   standing go-ahead covers the `submit` and `merge` pushes (`AGENTS.md` §4). Ask before any other
   push.

## 2. Dispatch a task to a free slot

1. Run `cargo xtask tasks next --agent agent-<k>` and take the first task it offers. If it offers
   nothing, leave the slot free until a merge unblocks something.
2. Run `cargo xtask tasks claim <id> --agent agent-<k>`. That creates or reuses
   `../dalica.worktrees/agent-<k>` on a new branch from `origin/main`.
3. Write the output of `cargo xtask tasks env agent-<k>` to `<scratchpad>/agent-<k>/env.sh`.
   Shell state does not persist between tool calls, so the worker sources this file at the start of
   every command.
4. Spawn the worker with the Agent tool: general-purpose, in the background, named
   `agent-<k> <id>`. Use the **worker prompt** in §5, with every `<…>` filled in. Do not use the
   Agent tool's own worktree isolation; the board's worktree is the one to use.
5. Note the start time for each task: slot, id and time. These feed the review's parallel-agent
   numbers.

Fill every free slot this way, then wait. Workers report back when they finish. Never poll in a
loop or sleep. If nothing is running and nothing can be merged, run `cargo xtask tasks status`
once and report.

## 3. When a worker returns

1. Read its report and run `cargo xtask tasks status`.
2. **Submitted (In Review).** Review the PR before merging:
   - Changed files (`git diff --name-only origin/main...<branch>`) must stay within the task's
     `files`, the shared registration files (`SHARED_FILES`), and its own crates' `Cargo.toml`
     and `lib.rs` `mod` lines.
   - The new behaviour has tests.
   - The task's ✅ and parity rows are updated.
   - A dev-log entry exists.
   - The PR body gives evidence for each line of the *done when*.
   - New, changed or deleted fingerprint baselines show their images on the PR and the commit body
     says why. They need no label or approval before the merge: `tasks merge` records them in the
     dev log and the maintainer approves them in bulk at the next review (`/milestone-review`).
     Never wait for the maintainer on a baseline mid-run.
   - If anything is missing, send it back (step 5) instead of merging.
3. **Merge one at a time:** `cargo xtask tasks merge <id>`. It rebases, runs the local `ci`,
   squash-merges and syncs; it does not wait for the PR's GitHub checks. Run it in the background
   and keep dispatching to free slots meanwhile. GitHub's three-OS run then checks `main`, and
   `tasks merge` refuses while the latest finished run on `main` is red.
   - Conflict only in `Cargo.lock`: re-resolve it (`cargo update -w`, never hand-merge).
   - Any other conflict, or red CI after the rebase: send the task back to its slot.
4. After each merge, fill free slots again (§2). A merge may have made new tasks Ready.
5. **Sending a task back.** If the worker is still addressable, continue it with SendMessage, giving
   the conflict or CI output. Otherwise spawn a new worker on the same slot and branch. Tell it the
   claim is already its own and to read `git log origin/main..HEAD` and the PR first.
6. **Blocked.** The worker ran `tasks block`, or it needs a file outside its list. Ask the
   maintainer straight away with the AskUserQuestion tool, while the other workers keep going:
   - Quote the worker's question word for word in the question text.
   - Offer 2–4 concrete answers, your recommendation first.
   - Workers cannot ask the maintainer themselves, so the lead always relays the question. Pass
     the answer back to the worker with SendMessage, or put it in the prompt of the task's next
     worker.
   Release the task (`tasks release <id> --reason …`) if it can't continue.
7. **Crashed or returned without submitting.** Check the worktree (`git status`,
   `git log origin/main..HEAD`), then resume it as in step 5 or release it.
8. **`main` is red** on GitHub CI after a merge (usually Windows or macOS):
   - Stop merging; workers keep working.
   - Find the merge that broke it: `gh run list --branch main`, then the failing log.
   - Fix it as the next task: a worker on a `task/<id>-fix` branch, or revert through a PR.
   - Merge the fix; `tasks merge` accepts merges again once `main` is green.
   - Note it for the check-in page. No need to stop for the maintainer.
9. Keep a short running table in the dev log, using `cargo xtask tasks log <check-in id> "…" --agent agent-1` at each
   merge or incident: claim → submit → merge times, conflicts (which files), CI failures,
   send-backs.
10. Note numbers for `docs/status.md`. Do not edit it here; it is written by the check-in
    (`/milestone-review`).

## 4. Plan ahead, and stop at the next review task

- **Plan ahead.** As soon as a milestone's first wave is running, make sure the next milestone's
  task blocks are on `main`. Each milestone carries an `owner: lead` task for this ("Lead: task
  blocks for M<n+1>"); if it is missing, add one.
  - Write the blocks from the milestone's section and the spec: `after`, `files`, `scope`,
    `done when`, plus waves.
  - The first tasks list the work they build on. List a gate only after a release.
  - The milestone ends with an `M<n+1>.R` review task, and a big one has a mid-milestone
    `M<n+1>.A`; every review task is `owner: maintainer` (the parser refuses any other owner).
    M6, M10 and M15 end with release gates.
  - Merge the blocks like any task; `tasks sync` then creates their issues. No approval is needed.
- **Review task: stop.** When the next tasks wait on an open review task (`M<n>.A`, `M<n>.R`,
  release gates included), `tasks next` offers nothing listed after it:
  - Let the running tasks finish; review and merge them as usual. Claim nothing listed after the
    review.
  - Run `/milestone-review M<n>.<X>` to prepare it: the check-in page, *Where we stand* and
    `docs/status.md` on a `review/M<n>.<X>` branch, a pull request, both linked on the issue (for a
    release gate also the checklist and the sign-off questions).
  - Never merge that pull request, never close the review issue, never `tasks merge` it. Then end
    the run.
- **Final message** (at a review task, or when the maintainer stops the run):
  - the review waiting: its pull request and check-in page links, and "Merge the PR and close
    #<issue> to approve; then run `/milestone-start` again to continue";
  - tasks merged per milestone, and any blocked or released;
  - wall-clock time;
  - one line per incident.

## 5. Worker prompt (fill in every `<…>`)

````text
You are agent-<k>, a worker on Dalica, an open-source pure-Rust photo organizer. Task <id> "<title>"
(issue #<issue>) is already claimed for you on branch <branch> in the worktree <worktree>. You do
this one task, submit it and report back. You never merge.

Shell state does not persist between commands, so start every shell command with:
  source <scratchpad>/agent-<k>/env.sh && cd "$DALICA_WORKTREE" && …
It sets your target dir, build jobs (3), control port ($DALICA_CONTROL_PORT) and scratch dir.

Read first, in the worktree: AGENTS.md (all of it), your task's block in docs/roadmap.md, the spec
files it points to (docs/README.md says which), and the README of each crate you touch.
If you are resuming work, also read `git log origin/main..HEAD` and the PR.

Rules:
- Change only the files in your task's `files`. You may also make registration edits in the shared
  files (root Cargo.toml, Cargo.lock, xtask/src/main.rs, tests/acceptance/Cargo.toml) and in your
  own crates' Cargo.toml and lib.rs `mod` lines; keep those small and re-read the file right before
  editing. If you need any other file, stop and report it. Do not edit it.
- Never edit docs/status.md, another task's block, or files of another in-progress task. If main
  or another agent's work breaks your build, wait and retry once; if it is still broken, report it.
- Follow AGENTS.md §3: no panics in non-test code, a test for every new behaviour, a
  graceful-failure test for every new command, never lower a metric, never weaken a gate.
- Use only your own control port. Never send OS-level synthetic input. Never rm -rf shared paths.
- Decide technical questions yourself (AGENTS.md §4): follow the spec; where it is silent, do what
  LightCraft or PhotoCraft did; record each decision in the commit body (what, why, the
  alternative). Only AGENTS.md §4's short list (licence or C-dependency exceptions, risks to users'
  data, loosening a gate, dropping a feature, reserved decisions) is a reason to run
  `cargo xtask tasks block <id> "<question>"` and report.

Finish with AGENTS.md §5, in your worktree:
  cargo fmt --all; cargo clippy -p <crates> --all-targets -- -D warnings; cargo test -p <crates>;
  cargo xtask ci; plus panic_hunt if you added or changed commands; `cargo xtask parity --write` if
  parity rows changed; screenshots at 1400×900 and 900×600 if the change is user-visible. If you
  added or re-blessed fingerprints (DALICA_BLESS=1), look at the diff images, attach the new PNG or
  the diff images to the PR and say why in the commit body; do not wait for an approval (the
  maintainer approves baselines in bulk at the next review).
  Tick your task's ✅ in docs/roadmap.md and update your parity rows and any spec section you
  clarified. Commit with the subject `<id>: <summary>`, a body covering what was tested, numbers
  and decisions, and the attribution line your environment requires.
  Then run `cargo xtask tasks log <id> "Landed: …  Open: …  Numbers: …"` and
  `cargo xtask tasks submit <id>`. The PR body gives evidence for each line of the *done when*.

Report back in this shape, and keep it short:
  status: submitted | blocked | needs-file | failed
  pr: <url>
  done when: one line of evidence per item
  numbers: …
  decisions not in the spec: …
  open: …
  files outside `files`: none | <list and why>
````
