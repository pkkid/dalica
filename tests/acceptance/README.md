# dalica-acceptance

One acceptance test file per milestone (`m<n>.rs`) plus its manual checks (`m<n>.md`), run by
`cargo xtask accept M<n>` (`docs/roadmap.md` → How milestones work). Nothing may depend on this
crate. `src/lib.rs` holds helpers shared by the milestone tests.

- **Test it alone:** `cargo test -p dalica-acceptance` (every milestone), or
  `cargo test -p dalica-acceptance --test m0_skeleton` (one file).
