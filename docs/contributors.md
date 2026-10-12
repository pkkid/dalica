# Contributors in the About window

Adapted from LightCraft `docs/contributors.md` (revision 88ea1a8) and PhotoCraft
`docs/contributors.md` (revision 59684266); MIT OR Apache-2.0.

> Status: **M6.** `cargo xtask contributors` and its `--check` exist (M6.11) and the first
> `contributors.json` is committed; the About window that shows them is M6.18.

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
repository, so Dalica has its own **`cargo xtask contributors`** with the same behaviour and the
same file shape (schema 1, the one the Craft apps' `build.rs` reads):

```sh
cargo xtask contributors               # verify people.toml, then regenerate contributors.json; needs git and gh
cargo xtask contributors --check       # verify people.toml only, offline (also run by `cargo xtask ci`)
cargo xtask contributors --ref <rev>   # count another ref (default: origin/main, else main)
```

- **Stats** come from `git log` on `main` (`origin/main` when it exists, since GitHub must know the
  commits; run `git fetch` first). Merge commits are not counted themselves; the commits they bring
  are. Per commit: lines added and deleted, and binary files added and removed (from `--numstat`
  and `--raw`, renames off); its date is the committer date in UTC.
- **Usernames** come from GitHub through `gh` (read-only): the account GitHub links each commit to,
  else the author of the merged pull request whose merge commit it is; a commit with neither, or
  only a bot (`…[bot]`, `app/…`), counts in `unattributed_commits`. A contributor's PRs are the pull
  requests merged into `main` whose merge commit is in that history. Every `gh` query keeps only
  shas, usernames and profile names, so no author name or email reaches the tool, and none is
  written.
- **Models** are counted from `Co-Authored-By` trailers (once per commit, with that commit's lines)
  when the name and the address's domain belong to a known model vendor (`Claude` at
  `anthropic.com`, `Copilot` at GitHub's no-reply domain, `Codex`/`GPT`/`ChatGPT` at `openai.com`,
  `Gemini` at `google.com`, `Cursor` at `cursor.com`); any other trailer, a person's included, is
  ignored. `Claude Opus 5.5` is company Anthropic, model `Claude Opus`, version `5.5`, id
  `anthropic/claude-opus/5.5`; a trailing `(…)` note is left out.
- **`people.toml`** is TOML restricted to what its template uses: `[people.<username>]` tables (a
  GitHub username) with `real_name`, `display_name` and `consent_source` (one-line strings) and
  `sync_github_name` (`true`/`false`), and comments. Malformed TOML, other tables, unknown keys, a
  wrong type, a key set twice and a repeated entry each fail, with their line.
- **Ownership:** the commit that last added an entry (it was absent from the file before) must be
  by that user, or the entry must have a `consent_source`; otherwise both modes fail. The full run
  knows each commit's username from GitHub; `--check` is offline and can tell only from a GitHub
  no-reply author address, so it skips what it cannot tell (and a shallow clone, as in GitHub CI)
  and says so. `people_entry` in the JSON is `true` only for an entry that passed (by owner or by
  consent); the About window shows names only for those. `github_name` is fetched only for such an
  entry with `sync_github_name = true`.
- Contributors are sorted by commits, then PRs, then username; models by commits, then id.
- Commit the updated `contributors/contributors.json` on its own (`M<n>.x: refresh contributors`).
  Neither mode ever writes `people.toml`.
