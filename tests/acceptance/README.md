# dalica-acceptance

One acceptance test file per milestone (`m<n>.rs`) plus its manual checks (`m<n>.md`), run by
`cargo xtask accept M<n>` (`docs/roadmap.md` → How milestones work). Nothing may depend on this
crate. `src/lib.rs` holds helpers shared by the milestone tests.

- **Test it alone:** `cargo test -p dalica-acceptance` (every milestone), or
  `cargo test -p dalica-acceptance --test m0_skeleton` (one file).
- **In `cargo xtask ci`:** the workspace test step leaves this package out; each finished
  milestone's `m<n>` runs once in its `accept M<n>` step and the other targets in `test acceptance`,
  with `DALICA_SUITES_RAN=1` so the crate suites the test step already ran are not run again
  (`docs/development.md` §3). Nested cargo runs go through `cargo()`, which never passes
  `DALICA_BLESS` down.
