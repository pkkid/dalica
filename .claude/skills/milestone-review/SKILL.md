---
name: milestone-review
description: Prepare a review task (M<n>.A, M<n>.R, and the release gates M6.R, M10.R, M15.R) for the maintainer - check-in page, docs on a review branch, a pull request - then stop. Never merges the PR or closes the review issue; only the maintainer does. Usually run by /milestone-start when the run reaches a review; the maintainer may run it directly.
argument-hint: "[M<n>.R | M<n>.A]  (default: the first open review task)"
---

# Prepare a review task

Target: **$ARGUMENTS**. If empty, use the first open `M<n>.A` / `M<n>.R` in
`cargo xtask tasks status`.

You are the lead (`agent-1`, `docs/task-board.md` §6). Every review task is a **stop** (maintainer
rule, 2026-10-10; `docs/roadmap.md` → How milestones work): it is `owner: maintainer`, and nothing
listed after it is claimed until the maintainer closes it. Your job is to **prepare** it so the
maintainer can decide quickly: gather the facts, publish a check-in page, put the docs on a review
branch, open a pull request, and stop.

- **You never merge the review's pull request and never close its issue.** Only the maintainer
  does. Do not `tasks claim`, `tasks submit` or `tasks merge` a review (they refuse), and do not
  use `gh pr merge` or `gh issue close` on it.
- **Release gates** (M6.R, M10.R, M15.R) are prepared the same way, plus the release checklist and
  the sign-off questions of §5.

**Scope.** For `M<n>.R`: every task of milestone M<n>. If the milestone had a mid-milestone review
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
`## <YYYY-MM-DD HH:MM> · agent-1 · M<n>.R Check-in` (or `M<n>.A`, or `Release gate`). Keep it short:

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

The page is linked on the review issue together with the pull request (§4).

## 4. Docs on a review branch, a pull request, and stop

The review's id is `M<n>.<X>` (`M2.R`, `M3.A`, …) and its issue `#<issue>`.

1. Make a branch `review/M<n>.<X>` from `origin/main` in a worktree of your own (for example
   `git worktree add ../dalica.worktrees/review-M<n>.<X> -b review/M<n>.<X> origin/main`, with
   `CARGO_TARGET_DIR` at `target/agent-1`). There:
   - write the dated *Where we stand* paragraph in `docs/roadmap.md` and the numbers in
     `docs/status.md`;
   - apply any reversals the maintainer asked for since the last review;
   - tick the review's ✅ in `docs/roadmap.md`, so merging the PR is the approval.
2. Commit as `M<n>.<X>: check-in` (body: what the page covers, the numbers, the attribution line),
   run `cargo xtask ci` there, and `cargo xtask tasks log M<n>.<X> "…" --agent agent-1`.
3. Push the branch (`git push -u origin review/M<n>.<X>`) and open the pull request with
   `gh pr create --base main --head review/M<n>.<X> --title "M<n>.<X>: check-in"`. Its body:
   - the check-in page's link and a three-line summary (what to try, the decisions to look at);
   - the line "Merge this PR and close #<issue> to approve; the next run starts after that."
     (GitHub reads "close #<issue>" in a PR body as a closing keyword, so the maintainer's merge
     also closes the issue.)
   - the attribution line the environment requires.
4. Comment both links on the review issue:
   `gh issue comment <issue> --body "Check-in page: <page url>  Pull request: <pr url>"`.
5. **Stop.** Never merge that PR, never close the issue, and do not claim anything listed after the
   review. Report the PR and the page in one short message and tell the maintainer: after merging
   the PR and closing #<issue>, run `/milestone-start` again. If `/milestone-start` is running, it
   ends its run with that message.

## 5. Release gates: the checklist and the sign-off questions

For a release gate (M6.R, M10.R, M15.R), before §4's step 5:

- Add the release checklist (`docs/releasing.md`) to the page: packages built per OS, install
  steps, known issues.
- Ask with the **AskUserQuestion** tool, recommended option first:
  - one question to sign off the release: ship / ship after listed fixes / hold;
  - questions for any decision the maintainer must make (`AGENTS.md` §4's short list).
  - Up to 4 questions per call; keep asking until everything is answered.
- Apply the answers on the `review/M<n>.R` branch (or on a separate PR that the maintainer also
  merges) and say on the review PR what changed.
- Still never merge the review PR or close the gate issue: the maintainer does both, then runs
  `/milestone-start M<n+1>`.

The names rule applies everywhere: never name the reference application, in the dev log, the page
or a commit.
