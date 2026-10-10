---
name: milestone-review
description: Run a milestone check-in (M<n>.R or M<n>.A owned by the lead; publish a page for the maintainer and keep going) or a release gate (M6.R, M10.R, M15.R; stop for the maintainer's sign-off). Usually run by /milestone-start; the maintainer may run it directly.
argument-hint: "[M<n>.R | M<n>.A]  (default: the first open check-in or release gate)"
disable-model-invocation: true
---

# Milestone check-in or release gate

Target: **$ARGUMENTS**. If empty, use the first open `M<n>.A` / `M<n>.R` in
`cargo xtask tasks status`.

You are the lead (`agent-1`, `docs/task-board.md` §6). The roadmap (*How milestones work*) gives two
kinds:

- **Check-in** (the block says `owner: lead`). Nothing waits for it. Gather the facts, publish a
  check-in page, write the docs, merge, and **keep going**. Never stop to ask; the maintainer reads
  the page when convenient and reverses anything by replying.
- **Release gate** (`owner: maintainer`: M6.R, M10.R, M15.R). Do the same, then **stop**: ask the
  maintainer to sign off, and apply their answers.

**Scope.** For `M<n>.R`: every task of milestone M<n>. If the milestone had a mid-milestone check-in
(`M<n>.A`), cover the whole milestone but say briefly what that one already showed. For `M<n>.A`:
the tasks from the start of M<n> up to it.

## 1. Gather the facts (read-only)

- **Plan:** the milestone's section and task blocks in `docs/roadmap.md`, *Reserved decisions*,
  and the previous check-in's dev-log entry. Check that any reversals the maintainer asked for
  since then were applied.
- **What happened:**
  - the dev-log entries since the previous check-in (their `Landed`, `Open` and `Numbers` lines);
  - `git log` of the `M<n>.` commits on `origin/main`, whose bodies hold the decisions;
  - merged PRs and their bodies (`gh pr list --state merged --search "M<n>."`).
- **Board:** `cargo xtask tasks status`; open issues labelled `needs-maintainer` or `stale`;
  leftover worktrees (`git worktree list`).
- **Loose ends in code:** `TODO(maintainer)` comments, `#[ignore]`d tests, fingerprint blesses.
- **Measurements** on a clean checkout of `origin/main` at one named commit, with a target dir in
  your scratchpad. Each number gets a unit, the machine and the commit:
  - `cargo xtask ci` (cold and warm), and the test count;
  - `cargo xtask accept M<k>` for every finished milestone and this one;
  - `panic_hunt`, if there are commands;
  - GitHub CI per OS on the latest `main` run;
  - the target dir size, free disk, and the `cargo xtask parity` counts;
  - every number the milestone's *Done when* lines name, plus benchmarks once they exist.
  - With parallel agents, also record: tasks per agent, time from claim to merge, rebase
    conflicts (which files), CI failures on `main`, send-backs.
  - Write "not measured" with the reason rather than estimate.
- **Screenshots** (whenever the app's look changed): 1400 × 900 and 900 × 600 through
  `dalica-cli snapshot` or the control channel, plus a close-up of each new feature. Compare
  them with `docs/ui-layout.md`.

## 2. Write it down

Append one entry to `log/devlog.md` in the **main checkout**, headed
`## <YYYY-MM-DD HH:MM> · agent-1 · M<n>.R Check-in` (or `Release gate`). Keep it short:

1. **What landed:** one line per task, with evidence against its *done when*, numbers and the PR.
   Then the milestone's numbers against the previous check-in's.
2. **Acceptance:** the `accept` result and each line of *Done when*, met or not. List any parity
   ✅ that was not measured, and downgrade it to 🟡.
3. **Decisions made:** numbered, from commit bodies and dev-log entries (every `DEFERRED:` line
   since the last check-in is one; it may have been answered meanwhile). Each one gets what was
   decided, why, the alternative and the PR. Say which decisions default to what LightCraft or
   PhotoCraft did.
4. **Surprises and process friction:** short, each with what you changed or will change.
5. **Open items:** every `Open:` line and `TODO(maintainer)`. Each gets the task that takes it, or
   the reason it is dropped.
6. **Next:** the next milestone's task blocks. Check that they are on `main` already (the lead writes
   them ahead). If they are not, write them now, before anything else. Add the parallelism
   report: waves, how many tasks can run at once, and shared-file hotspots.

## 3. Publish the check-in page

Publish one artifact page per check-in (load the `artifact-design` skill first). Title it
`Dalica M<n> check-in`; for `M<n>.A`, `Dalica M<n>.A check-in`. It is the maintainer's view of the
milestone, so lead with the app:

- **Try it:** exact commands, such as `cargo run -p dalica -- --demo`, and what to click and look
  for, step by step.
- **Screenshots** at both sizes, with the new features shown close up.
- **What changed** for a user, in plain words; not the task list.
- **Decisions made:** the numbered list from §2. Under it: "To reverse one, reply with its number,
  or comment on the page".
- **Numbers:** the few that matter (speed budgets met, test count, CI time per OS).
- **Next:** what the next milestone builds, in two or three lines.

Link the page in a comment on the check-in issue: `gh issue comment <n> --body "Check-in page:
<url>"`.

## 4. Docs, tick and merge (a check-in ends here)

1. `cargo xtask tasks claim M<n>.R --agent agent-1` (or `.A`).
2. In its worktree:
   - write the dated *Where we stand* paragraph in `docs/roadmap.md` and the numbers in
     `docs/status.md`;
   - apply any reversals the maintainer asked for since the last check-in;
   - tick the check-in's ✅.
3. `cargo xtask ci`, `cargo xtask tasks log M<n>.R "…"`, `cargo xtask tasks submit M<n>.R`,
   `cargo xtask tasks merge M<n>.R`.
4. For a **check-in**: report the page's link in one line and go back to work. That means the
   `/milestone-start` loop if it is running, or a pointer to `/milestone-start` if not.

## 5. Release gates only: stop for the maintainer

After §1–§3, and the same docs as §4 without ticking the gate:

- Add the release checklist (`docs/releasing.md`) to the page: packages built per OS, install
  steps, known issues.
- Ask with the **AskUserQuestion** tool, recommended option first:
  - one question to sign off the release: ship / ship after listed fixes / hold;
  - questions for any decision the maintainer must make (`AGENTS.md` §4's short list).
  - Up to 4 questions per call; keep asking until everything is answered.
- Apply the answers on a branch and merge it.
- Once the maintainer closes the gate issue, tick it ✅ and run `cargo xtask tasks sync`; the next
  milestone's tasks become Ready. Tell the maintainer to run `/milestone-start M<n+1>`.

The names rule applies everywhere: never name the reference application, in the dev log, the page
or a commit.
