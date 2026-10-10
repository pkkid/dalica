# Contributors in the About window

Adapted from LightCraft `docs/contributors.md` (revision 88ea1a8) and PhotoCraft
`docs/contributors.md` (revision 59684266); MIT OR Apache-2.0.

> Status: **Planned (M11).** Nothing below exists yet; this page records the decision so the
> files can be kept correct from the first community contribution.

**About ▸ Contributors** credits everyone who contributed to Dalica, and **About ▸ Models** credits
the AI models named in `Co-Authored-By` trailers. Dalica follows the same contributor-credit
standard as the Craft apps (LightCraft, PhotoCraft); this page is Dalica's own copy of it.

## Decision

- Two files, both **compiled into the binary** by the UI crate's `build.rs` (nothing is read from
  disk or the network at run time):
  - `contributors/contributors.json`: commit stats per GitHub username. **Generated** by
    `cargo xtask contributors` (below); never hand-edit.
  - [`contributors/people.toml`](../contributors/people.toml): the names people chose for
    themselves. **Each contributor adds only their own entry**, in their own PR.
- Each contributor gets one line: GitHub username (always), display name (opt-in, or their synced
  GitHub profile name), real name (only if they told us), merged PRs, commits, lines added, lines
  deleted, binary assets added, binary assets removed, first and last commit date.
- The list is shown as a **grab bag** (names flowing as text) or a **table**, in the same order. It
  sorts by name, PRs, commits, lines added, lines deleted, line delta, binary assets added/removed,
  first or last commit date. The name toggle cycles **Username → Display name → Real name**; a
  missing name falls back to `@username`. Alphabetical sorting is case-insensitive and ignores the
  `@`.
- **Models** are counted from the `Co-Authored-By:` trailers of commits on `main` (model name and
  number of commits). Agents must therefore keep the trailer their environment requires
  ([`contributing.md`](contributing.md) → Pull requests).
- Git author names and emails are never recorded, shown or shipped; only GitHub usernames and the
  names people entered themselves.

## In Dalica

- Help ▸ About Dalica (on macOS, Dalica ▸ About Dalica) opens a dialog with the tabs
  **About · Contributors · Models** ([`menus-and-shortcuts.md`](menus-and-shortcuts.md)). The tab
  and the list view are UI state, so agents can switch them through the control channel
  (`ui.clickWidget` on the tab buttons) and screenshot the result.
- Labels are translated like the rest of the shell (`tl!`, see [`localization.md`](localization.md));
  people's names are never translated or transliterated.

## Credit yourself

Add your entry to `contributors/people.toml` in a PR you commit yourself (every field optional):

```toml
[people.your-github-username]
real_name = "Your Name"
display_name = "Nickname"
sync_github_name = true   # use the name on your public GitHub profile as display name
```

Names show after the next refresh: the refresh checks that each entry was committed by its own
user (or by a maintainer with a `consent_source` saying where the person asked). Delete your entry
to remove your names.

## Agents working for a contributor

- When you prepare a PR, check whether your human's GitHub username has a `[people.<username>]`
  entry in `contributors/people.toml`.
- If not, **ask them once** whether they want to be credited by more than their username: a real
  name, a display name, and/or their public GitHub profile name (`sync_github_name = true`).
- If yes, add **only their own** entry (copy the template at the top of the file) and include it in
  their PR, committed as them. If no, change nothing: they are credited as `@username` anyway.
- **Never guess names.** Never add, edit or copy anyone else's entry, and never take a name from git
  config, commit authors, email addresses or GitHub profiles unless the person told you to (and
  then only through `sync_github_name`). Never hand-edit `contributors.json`.

## Refreshing (maintainers)

The Craft apps generate `contributors.json` with a maintainer script that lives in a private shared
repository, so Dalica cannot use it. Dalica will get its own **`cargo xtask contributors`**
(planned, M11) with the same behaviour:

```sh
cargo xtask contributors           # regenerate contributors.json + verify people.toml; needs git and gh
cargo xtask contributors --check   # verify people.toml only (also run by `cargo xtask ci`)
```

- Stats come from `git log` on `main` and, for merged PRs and usernames, the GitHub API through
  `gh`; commits are mapped to GitHub usernames, never stored by author name or email.
- `--check` fails on unknown keys, an entry whose file history shows it was added by someone other
  than its owner without a `consent_source`, and malformed TOML.
- Commit the updated `contributors/contributors.json` on its own (`M11.x: refresh contributors`).
