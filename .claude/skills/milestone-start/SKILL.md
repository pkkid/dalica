---
name: milestone-start
description: Start a milestone (M2 and later) with this session as the lead (agent-1) and parallel worker subagents, one per task; run it until the next review gate. Run only when the maintainer asks.
argument-hint: "[M<n>] [workers, default 7]"
disable-model-invocation: true
---

# Start a milestone

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
- Stop at the next review gate. That is either a checkpoint `M<n>.A` or the end of the milestone,
  `M<n>.R`.

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
   - The previous gate is closed: its first tasks show Ready in `cargo xtask tasks status`.
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
   - Find where the gates fall.
   - If fewer tasks can run at once than there are workers, use fewer workers and say so.
7. Show the maintainer a short plan: milestone, worker count, waves, gates where the run will stop,
   and any preflight warning. Then start; the command itself is the go-ahead. The maintainer's
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
   - If anything is missing, send it back (step 5) instead of merging.
3. **Merge one at a time:** `cargo xtask tasks merge <id>`. It rebases, runs `ci`, waits for
   GitHub CI, squash-merges and syncs. Run it in the background and keep dispatching to free slots
   meanwhile.
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
8. **`main` is red after a merge.** Stop dispatching. Find the merge that broke it. Fix it on a
   branch, or revert it through a PR. Tell the maintainer. Resume when `main` is green.
9. Keep a short running table in the dev log, using `cargo xtask tasks log M<n>.R "…" --agent agent-1` at each
   merge or incident: claim → submit → merge times, conflicts (which files), CI failures,
   send-backs.
10. Note numbers for `docs/status.md`. Do not edit it here; it is written in the review commit
    (`/milestone-review`).

## 4. Stop at the gate

- **Checkpoint `M<n>.A`.** Once every task before it has merged, stop dispatching. Run the
  checkpoint's acceptance check if there is one. Then tell the maintainer to run
  `/milestone-review M<n>.A`.
- **Last task merged.** Run `cargo xtask accept M<n>` on `origin/main`. Confirm every worktree is
  clean and no task is In Progress or In Review. Tell the maintainer to run
  `/milestone-review M<n>.R`.
- Never claim a task beyond the gate, and never a task of the next milestone. When the gate is
  closed, the maintainer runs `/milestone-start` again.
- Final message: tasks merged, any blocked or released, wall-clock time, a one-line note per
  incident, and the gate to review.

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
- A decision only the maintainer can make (a reserved decision, a gap in the spec): run
  `cargo xtask tasks block <id> "<question>"` and report. Otherwise decide yourself and record the
  decision in the commit body.

Finish with AGENTS.md §5, in your worktree:
  cargo fmt --all; cargo clippy -p <crates> --all-targets -- -D warnings; cargo test -p <crates>;
  cargo xtask ci; plus panic_hunt if you added or changed commands; `cargo xtask parity --write` if
  parity rows changed; screenshots at 1400×900 and 900×600 if the change is user-visible.
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
