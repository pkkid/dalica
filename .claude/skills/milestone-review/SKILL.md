---
name: milestone-review
description: Prepare a review task (M<n>.R, and the release gates M6.R, M10.R, M15.R) for the maintainer - the few questions only they can answer (asked here), a UI changelog with before/after screenshots, a check-in page, docs on a review branch, a pull request - then stop. Never merges the PR or closes the review issue; only the maintainer does. Usually run by /milestone-start when the run reaches a review; the maintainer may run it directly.
argument-hint: "[M<n>.R]  (default: the first open review task)"
---

# Prepare a review task

Target: **$ARGUMENTS**. If empty, use the first open `M<n>.R` in
`cargo xtask tasks status`.

You are the lead (`agent-1`, `docs/task-board.md` §6). Every review task is a **stop** (maintainer
rule, 2026-10-10; `docs/roadmap.md` → How milestones work): it is `owner: maintainer`, and nothing
listed after it is claimed until the maintainer closes it. Your job is to **prepare** it so the
maintainer can decide quickly: gather the facts, publish a check-in page, ask the questions, put
the docs and the answers on a review branch, open a pull request, and stop.

The milestone ran without the maintainer (maintainer's process, 2026-10-10): every decision that
needed them mid-milestone was recorded instead of asked. The review is where they are asked, so:

- **Questions for you** come first, and as few as possible: only what the maintainer must decide
  (blocked tasks' questions, `AGENTS.md` §4 short-list items, risky or hard-to-reverse calls). Each
  has options and your recommendation. Everything else is a **decision made**, listed collapsed
  and reversible by replying with its number.
- Then a **UI changelog**: every user-visible change since the last review, so the maintainer can
  check the UI by eye.
- The maintainer answers the questions, checks the UI, merges the review PR (which closes
  `M<n>.R`) and runs `/milestone-start` for the next milestone.

- **You never merge the review's pull request and never close its issue.** Only the maintainer
  does. Do not `tasks claim`, `tasks submit` or `tasks merge` a review (they refuse), and do not
  use `gh pr merge` or `gh issue close` on it.
- **Release gates** (M6.R, M10.R, M15.R) are prepared the same way, plus the release checklist and
  the sign-off questions of §5.

**Scope.** For `M<n>.R`: every task of milestone M<n>. A milestone has exactly one review, at its
end; there are no mid-milestone reviews (maintainer rule, 2026-10-10).

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
- **Questions for the maintainer:** every Blocked task's question (its `needs-maintainer` issue
  and the `QUESTION:` dev-log lines since the previous check-in), any `AGENTS.md` §4 short-list
  item and any risky or hard-to-reverse call found in the commit bodies or `DEFERRED:` lines.
  Keep a question only if the maintainer must make it; turn the rest into decisions made. For
  each: the question in one sentence, 2–4 options, your recommendation first, and what waits on
  it.
- **UI changes:** every user-visible change merged since the previous review, from the PR bodies,
  the parity rows ticked and the commit subjects of `ui`/`ui-kit` tasks. For each: what changed,
  the task and PR, where to click to see it, and a before and after screenshot (before: the
  previous review's commit; after: this commit; both window sizes where the layout changed).
- **Loose ends in code:** `TODO(maintainer)` comments, `#[ignore]`d tests, fingerprint blesses.
- **Fingerprints to approve** (`docs/task-board.md` §7): baselines merge with their task and the
  maintainer approves them in bulk here. Collect every `FINGERPRINTS:` line that `tasks merge`
  wrote to `log/devlog.md` (main checkout) since the previous review's check-in entry:
  `grep -n '^FINGERPRINTS:' log/devlog.md`, keeping the lines below that entry's heading. Each line
  reads `FINGERPRINTS: <task id> #<PR> added|changed|deleted <paths>`. For each one:
  - the PR link (`https://github.com/pkkid/dalica/pull/<PR>`) and the commit body's reason;
  - the image(s): run the fingerprint's test in your clean checkout of `origin/main` (the
    measurement checkout below); it writes what it rendered under that checkout's
    `target/test-output/<crate>/` (never commit those PNGs). For a changed baseline also run it at
    the commit before the merge, for the before image. Or link the images attached to the PR. For a
    deleted baseline, say what replaced it.
  - Cross-check with `git log --name-status origin/main` over `**/fingerprints/*.json` since the
    previous review, in case a merge failed to record its lines; add any it missed.
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

0. **Questions:** the list from §1, numbered, each with its options and recommendation; the
   answers are added in §4.
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
   them ahead). If they are not, write them now, before anything else. They become issues only
   when the maintainer closes this review (any task `pulled` forward already has one). Add the
   parallelism report: waves, how many tasks can run at once, and shared-file hotspots.

## 3. Publish the check-in page

Publish one artifact page per check-in (load the `artifact-design` skill first). Title it
`Dalica M<n> check-in`. It is the maintainer's view of the milestone, in this order:

- **Questions for you:** the numbered questions from §2, each with its options, the
  recommendation marked, and what waits on it. If there are none, say so in one line.
- **Decisions made:** the numbered list from §2, collapsed (a `<details>` block). Under it: "To
  reverse one, reply with its number, or comment on the page".
- **UI changelog:** every user-visible change from §1, each with its before and after screenshot
  (both sizes where the layout changed), where to click to see it, and the task and PR. Start the
  section with the demo command (such as `cargo run -p dalica -- --demo`) and the full window at
  both sizes. Say in plain words what changed for a user; not the task list.
- **Fingerprints to approve:** every baseline from §1 since the last review, grouped by task: the
  path, added/changed/deleted, the PR link, the reason, and the image(s) (before and after for a
  changed one). Number them so the maintainer can name the ones to redo. If there are none, say so.
- **Numbers:** the few that matter (speed budgets met, test count, CI time per OS).
- **Next:** what the next milestone builds, in two or three lines.

The page is linked on the review issue together with the pull request (§4).

## 4. Docs on a review branch, a pull request, and stop

The review's id is `M<n>.R` (`M2.R`, `M6.R`, …) and its issue `#<issue>`.

1. Make a branch `review/M<n>.R` from `origin/main` in the lead session's own worktree (no new
   worktree, no subagent; `CARGO_TARGET_DIR` at `target/lead`, never an agent's). **Everything the review
   changes goes in this one branch and its one pull request** (maintainer rule, 2026-10-10):
   docs, numbers, process fixes found during the milestone, answers applied. The one exception: a fix
   for red CI may be its own PR (the fix-red-main go-ahead). There:
   - write the dated *Where we stand* paragraph in `docs/roadmap.md` and the numbers in
     `docs/status.md`;
   - apply any reversals the maintainer asked for since the last review;
   - tick the review's ✅ in `docs/roadmap.md`, so merging the PR is the approval.
2. **Ask the questions** with the **AskUserQuestion** tool: the questions for the maintainer from
   §2, then the fingerprint question (step 6). Ask as few as possible, recommended option first,
   up to 4 per call, until everything is answered (a question the maintainer leaves open stays
   on the page and the PR as open). Then apply the answers on the review branch:
   - roadmap and docs edits the answers call for;
   - a fix-task block in `docs/roadmap.md` for each rejected item (`after:` the review);
   - for each blocked task, comment the answer on its issue, remove `needs-maintainer`
     (`gh issue edit <n> --remove-label needs-maintainer`, board housekeeping) so `sync` sets it
     Ready, and keep the answer for the next worker's prompt;
   - the answers in the check-in's dev-log entry, and on the page (republish it).
3. Commit as `M<n>.R: check-in` (body: what the page covers, the numbers, the attribution line),
   run `cargo xtask ci` there, and `cargo xtask tasks log M<n>.R "…" --agent agent-1`.
4. Push the branch (`git push -u origin review/M<n>.R`) and open the pull request with
   `gh pr create --base main --head review/M<n>.R --title "M<n>.R: check-in"`. Its body:
   - the check-in page's link and a three-line summary: the questions and their answers, the UI
     changes to check, the decisions to look at;
   - the number of fingerprint baselines to approve, with a pointer to that section of the page;
   - the line "Merge this PR and close #<issue> to approve; the next run starts after that."
     (GitHub reads "close #<issue>" in a PR body as a closing keyword, so the maintainer's merge
     also closes the issue.)
   - the attribution line the environment requires.
5. Comment both links on the review issue:
   `gh issue comment <issue> --body "Check-in page: <page url>  Pull request: <pr url>"`.
6. **The fingerprint question** (asked with the others in step 2, when §1 found any), recommended
   option first: "Approve all N fingerprint baselines" / "Redo some (list their numbers from the
   page)" / "Look later" (merging the review PR without naming any approves them all). Each
   rejected baseline becomes a fix task: write its block in `docs/roadmap.md` on the review branch
   (`after:` the review; `files`: the code that draws it, its test and its fingerprint; *done
   when*: the maintainer's objection is fixed and the new image is on its PR, to be approved at
   the next review), say on the review PR which numbers were rejected, and record the answer in
   the check-in's dev-log entry. Approved baselines need nothing more. Asking is not merging: the
   maintainer still merges the PR.
7. **Stop.** Never merge that PR, never close the issue, and do not claim anything listed after the
   review. Report the PR and the page in one short message and tell the maintainer what is left
   for them: check the UI with the page's changelog, answer any question still open, merge the PR
   (it closes #<issue>), then run `/milestone-start` for the next milestone. If `/milestone-start`
   is running, it ends its run with that message.

## 5. Release gates: the checklist and the sign-off questions

For a release gate (M6.R, M10.R, M15.R), before §4's step 7:

- Add the release checklist (`docs/releasing.md`) to the page: packages built per OS, install
  steps, known issues.
- Ask with the **AskUserQuestion** tool, recommended option first:
  - one question to sign off the release: ship / ship after listed fixes / hold;
  - the questions of §4's step 2 (decisions only the maintainer makes, the fingerprints), if they
    were not asked yet.
  - Up to 4 questions per call; keep asking until everything is answered.
- Apply the answers on the `review/M<n>.R` branch (or on a separate PR that the maintainer also
  merges) and say on the review PR what changed.
- Still never merge the review PR or close the gate issue: the maintainer does both, then runs
  `/milestone-start M<n+1>`.

The names rule applies everywhere: never name the reference application, in the dev log, the page
or a commit.
