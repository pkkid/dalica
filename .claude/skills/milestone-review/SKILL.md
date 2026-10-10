---
name: milestone-review
description: Prepare a milestone review (M<n>.R) or checkpoint review (M<n>.A) for the maintainer, write it to the dev log, show it, and wait for decisions. Run only when the maintainer asks.
argument-hint: "[M<n>.R | M<n>.A]  (default: the first open gate)"
disable-model-invocation: true
---

# Milestone review

Gate: **$ARGUMENTS**. If empty, use the first open gate (`M<n>.A` or `M<n>.R`) in
`cargo xtask tasks status`.

You are the lead (`agent-1`, `docs/task-board.md` §6) preparing this gate for the maintainer.
Prepare, write, show, then **stop and wait**. Do not apply any edit, tick any ✅, edit
`docs/status.md`, close the gate issue, or push until the maintainer has decided.

**Scope.** For `M<n>.R`: every task of milestone M<n>. If the milestone had a checkpoint (`M<n>.A`),
cover the whole milestone but say briefly what the checkpoint already reviewed. For `M<n>.A`: the
tasks from the start of M<n> up to the checkpoint.

## 1. Gather the facts (read-only)

- **Plan:** the milestone's section and task blocks in `docs/roadmap.md` (Goal, In, Out, Done when,
  each task's scope and *done when*), *Reserved decisions*, and the previous review's entry in
  `log/devlog.md`. Check that the previous review's decisions were applied.
- **What happened:** the dev-log entries since the previous gate (their `Landed`, `Open` and
  `Numbers` lines); `git log` of the `M<n>.` commits on `origin/main` (the commit bodies hold the
  decisions); merged PRs, their bodies and CI runs (`gh pr list --state merged --search "M<n>."`).
- **Board:** `cargo xtask tasks status`; `cargo xtask tasks sync --dry-run` (it should have nothing
  to write); open issues labelled `needs-maintainer`, `stale` or Blocked; leftover worktrees and
  branches (`git worktree list`).
- **Loose ends in code:** `TODO(maintainer)` comments, `#[ignore]`d tests, fingerprint blesses.
- **Measurements.** Take them on a clean checkout of `origin/main` at one commit (name it), with
  a target dir in your scratchpad. Each number gets a unit, the machine and the commit:
  - cold build, `cargo xtask ci` (cold and warm), `cargo test --workspace` (time and test count)
  - `cargo xtask accept M<k>` for every finished milestone and this one (result and time)
  - `cargo test -p dalica-engine --test panic_hunt -- --ignored`, if the engine has commands
  - GitHub CI duration per OS on the latest `main` run (`gh run list`, `gh run view`)
  - the target dir size, free disk, and `cargo xtask parity` summary counts
  - every number this milestone's *Done when* and its tasks' *done when* name, plus benchmarks
    (`cargo xtask bench`, `ui.inspect` → perf) once they exist
  - **Is there user-visible UI?** Take screenshots at 1400 × 900 and 900 × 600
    (`dalica-cli snapshot`) and compare them with `docs/ui-layout.md`.
  - **Did agents work in parallel?** Record tasks per agent, time from claim to submit, time
    spent In Review, rebase conflicts (which files), merge-time CI failures, stale releases and
    idle `next` waits.
  - If you cannot measure something, write "not measured" and the reason. Never estimate a
    number and present it as measured.

## 2. Write the review

Append one entry to `log/devlog.md` in the **main checkout** (not a worktree), headed
`## <YYYY-MM-DD HH:MM> · agent-1 · M<n>.R Milestone review (prepared for the maintainer)`
(or `M<n>.A Checkpoint review`). Use these sections:

0. **Decisions needed.** A numbered list of everything the maintainer must answer, pointing to the
   items below, so the reply can be "confirm all except 4 and 9".
1. **What landed.** A table, one row per task: what landed, evidence against its *done when*,
   numbers, and the commit or PR. Then a table of the milestone's numbers with the previous
   review's values and the change.
2. **Acceptance.** The `accept` result; each line of the milestone's *Done when*, met or not, with
   evidence. List the manual checks from `tests/acceptance/m<n>.md` as exact steps and commands for
   the maintainer to run. List the parity rows that changed; say whether each ✅ was measured, and
   propose 🟡 for any that was not.
3. **Surprises.** Where a task took longer than one session; where the spec was wrong, vague or
   missing something; where a *done when* was too weak or too strict.
4. **Decisions not in the spec**, collected from commit bodies and dev-log entries. Number each one
   and give: the decision, the commit, the alternative, and what reversing it would cost.
5. **Quality floors and safety.** Floors that moved (they may only rise). Any gate loosened, test
   ignored or fingerprint blessed (should be none; if there is one, give the justification).
   Crash or data-safety bugs found and their regression tests. New dependencies, with licence and
   any C-build exception. Changes relevant to security.
6. **Open items.** Every `Open:` line from this milestone's dev-log entries, plus every
   `TODO(maintainer)`, blocked or `needs-maintainer` issue and PR follow-up. Give each a proposed
   home: a task in the next milestone, a spec change, or drop it (and say why).
7. **Process friction.** The task board, the gates, the `AGENTS.md` rules, the dev log, parallel
   agents (with the numbers from §1), disk and time. Say what should change before the next
   milestone and what before more agents run in parallel.
8. **Next plan.** For `M<n>.R`: draft **every** task block for M<n+1> in roadmap format (`after`,
   `owner`, `files`, `scope`, `done when`), ready to paste into `docs/roadmap.md`:
   - The first tasks list `M<n>.R` in `after`.
   - The milestone ends with its own review gate `M<n+1>.R` (`owner: maintainer`), plus a
     checkpoint gate `M<n+1>.A` if the milestone is big.
   - These blocks are what `/milestone-start` runs; it writes no tasks of its own. For `M<n>.A`: the changes to M<n>'s remaining tasks. Add a
   parallelism report:
   - the waves, and how many tasks can be in progress at once given `files` overlap
   - the longest dependency chain
   - shared-file hotspots several tasks need
   - reserved decisions that block the plan
   - how this milestone's estimates compared with reality
9. **Proposed edits (not applied).** Concrete edits to task blocks, the spec (`docs/`),
   `AGENTS.md` and the `xtask` gates. Give each edit its file and section.
10. **Where we stand.** The dated paragraph proposed for `docs/roadmap.md`, and the numbers
    proposed for `docs/status.md`.

Write plainly and briefly. Lead with facts and numbers. The names rule applies: never name the
reference application anywhere in the review.

## 3. Show it and wait

Show the maintainer the whole entry. Print it in chat, or, if it is long, send that section as a
file. Also send any screenshots.

Then ask for the decisions with the **AskUserQuestion** tool, not in prose. Each call takes up to
4 questions, and each question takes 2–4 options; the maintainer can always pick "Other" and
type an answer.

- **Choices between real alternatives** (a gap in the spec, a reserved decision, which of two
  designs) get one question each:
  - The options are the alternatives, with your recommended option first and marked
    "(Recommended)".
  - Each option's description gives its cost or consequence in one line.
- **Confirm-or-reverse items** (decisions not in the spec, proposed edits) are grouped four to a
  question with `multiSelect: true`:
  - Phrase it as "Which of these should be reversed or changed? Select none to confirm all four."
  - Each option is one item: its number and a short label as the label, and the decision and its
    alternative as the description.
- For `M<n>.R`, one question approves the next milestone's task list as drafted in §8: approve,
  approve with listed changes, or rework.
- Ask in order of importance and keep asking until every item of *Decisions needed* has an
  answer.
- Any item the maintainer answers with "Other", or marks for change, gets a follow-up question or
  a short discussion before you treat it as decided.

Record the answers in the dev-log entry under *Decisions needed*, one line per item: confirmed,
reversed, or changed to what. Then go on to §4.

## 4. After the maintainer decides

Only when the maintainer replies:

1. Make a branch `task/M<n>.R-review` from `origin/main`, or `M<n>.A` for a checkpoint.
2. Apply the confirmed edits and the reversals. Write the *Where we stand* paragraph and the
   `status.md` numbers.
   - For `M<n>.R`, also write the approved M<n+1> task blocks into M<n+1>'s section of
     `docs/roadmap.md`, gates included.
   - `cargo xtask tasks sync --dry-run` must list one new issue per block and nothing else.
3. Run `cargo xtask ci`. It includes `tasks sync --check`, which fails if a block does not parse.
4. Commit as `M<n>.R: review decisions applied`. The body lists every decision and every edit.
5. `cargo xtask tasks log M<n>.R "Landed: …  Open: …"`.
6. Ask before pushing, unless the maintainer's standing go-ahead covers it (`AGENTS.md` §4).
7. Once the review commit is on `main` and the maintainer has closed the gate issue:
   - Tick the gate's ✅.
   - Run `cargo xtask tasks sync`. It creates the GitHub issues for the new task blocks on project
     3 and moves the first ones to Ready.
   - Confirm with `cargo xtask tasks status`, then tell the maintainer to run
     `/milestone-start M<n+1>`.
