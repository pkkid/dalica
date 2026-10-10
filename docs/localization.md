# Localisation

Adapted from PhotoCraft `docs/localization.md`, `docs/localization-ko.md` and the
`crates/ui-egui/src/i18n/` catalog format (revision 59684266), and LightCraft
`docs/localization.md` (revision 88ea1a8); MIT OR Apache-2.0.

> Status: **Planned (M15, parity row `SYS-I18N`).** From M0 on, every user-facing string is
> written as an English literal wrapped in `tl!` so the catalogs can be added later without
> touching call sites. The catalog loader, the language preference and the tests below land in M15.

Dalica follows the PhotoCraft i18n design (prior art only; our own code, see
[`architecture.md`](architecture.md) §9): **English source strings are the lookup keys**, UTF-8
**TSV catalogs** supply translations per language, and a missing entry falls back to English.

## Writing translatable code

- Wrap every user-facing literal: `tl!("Folder Manager")`. Command labels in the registry are
  English and are looked up the same way (the label is the key, the command id disambiguates).
- Text with values uses named placeholders: `tl!("{count} photos selected", count = n)`. Never build
  sentences by concatenating translated fragments; word order differs between languages.
- Count-sensitive text uses the plural form (`@plural` in the catalog, below), never `"photo(s)"`
  or an English `"s"` suffix.
- Dates and numbers (folder header dates, file sizes, capture times) go through
  the locale-aware formatting helpers, never hand-written `format!` patterns.
- Leave room: translations run up to 40 % longer than English. Buttons and tree rows must
  ellipsize or wrap rather than clip; check with a long language (German) in a snapshot.

## Catalog format

One file per language: `crates/ui/src/i18n/<code>.tsv`, embedded with `include_str!` (editing a
catalog requires a rebuild). Columns are separated by a TAB:

```text
context <TAB> source <TAB> translation
```

| Context | Meaning |
|---|---|
| empty | a plain UI string; `source` is the English text in the code |
| `@id` | a command id (e.g. `photo.star.toggle`): wins over the English text for that menu item, so one id can read differently from its twin with the same English label |
| `@plural` | `source` is `"one\|other"` (the English forms); the translation lists the language's forms separated by `\|`, in the order of that language's plural rule |
| anything else | a disambiguating context passed to `tl!(ctx = "…", "…")` |

- Escapes `\n`, `\t`, `\\` inside a column. `{name}` placeholders must appear in both columns.
- Lines starting with `#` and blank lines are ignored. Each file starts with a header comment that
  repeats these rules and the clean-room statement (below).
- Keep the trailing `…` of commands that open a dialog, and keep product and technology names
  (Dalica, JPEG, EXIF, XMP, GPS) in Latin letters.

## Languages and plural rules

Each language is registered in `i18n::LANGUAGES` with its BCP-47 code (also the value stored in
settings), its **endonym** (shown in the Language menu), its script (which decides the fallback
fonts), its plural rule, and a `complete_menus` flag:

| Plural rule | Languages (examples) |
|---|---|
| one form | Chinese, Japanese, Korean, Indonesian |
| one / other | English, German, Spanish, Italian, Dutch, Greek |
| singular for 0 and 1 / plural for 2+ | French, Brazilian Portuguese |
| one / few / many | Russian, Ukrainian, Polish (with their own few/many boundaries) |
| one / few / other | Czech |

The first languages are chosen by the maintainer when M15 starts; nothing is promised before then.

## Choosing the language

Tools ▸ Options ▸ General ▸ Language ([`menus-and-shortcuts.md`](menus-and-shortcuts.md) §5)
switches without restarting: the rest of the interface follows on its next draw, and open views,
the selection and the undo history are kept. The default is **System** (stored as `auto`), which
follows the system's UI language list and falls back to English; a manual choice is saved and wins on later launches.
Regional tags (`de-AT`, `pt-PT`, `zh-CN`) resolve to the closest registered catalog; Traditional
and Simplified Chinese stay distinct. `DALICA_LANGUAGE=<code>` overrides the setting for one
launch (tests, snapshots). Agents set the language with the normal settings command over the
control channel or MCP.

## What is not translated

- Command ids, automation widget ids, control-protocol and MCP method names, log messages.
- File and folder names, and everything the user typed: captions, tags, album names, people's
  names, folder descriptions, place names.
- Metadata written to files (XMP/IPTC keys and values), sidecar keys, export file-name templates'
  tokens.
- Low-level error details from decoders and the file system (the surrounding sentence is
  translated; the technical detail stays as reported).

## Clean-room translations

Translations are written from the **meaning of Dalica's English text**, using ordinary computing
and photography vocabulary in the target language. Never copy or consult the reference organizer's
localisation files, string tables or translated screenshots, or any other proprietary product's
localisation resources. Machine-assisted drafts are fine if a fluent reviewer checks them. Catalogs
are MIT OR Apache-2.0 like the code, with a row in [`ATTRIBUTION.md`](../ATTRIBUTION.md) and a
`LICENSE-translations.txt` next to them stating that they are original translations.

## Adding or maintaining a language

1. Copy an existing catalog's header to `crates/ui/src/i18n/<code>.tsv` and translate the meaning of
   the English keys, keeping placeholders, escapes and trailing `…`.
2. Register it in `i18n::LANGUAGES` (code, endonym, script, plural rule).
3. Set **`complete_menus: true`** once the catalog covers every enforced key. The tests then
   require that every menu label, every `tl!("…")` literal in the UI crate (test modules aside)
   and every generated preference and control label has an entry in that language.
4. Check the glyphs: the script decides which fallback faces the UI installs. A script without a
   face in the build shows boxes; the face is added to craft-fonts and the release pin, never
   committed here ([`assets.md`](assets.md) → Fonts).
5. Write a short per-language style guide, `docs/localization-<code>.md` (below).
6. Add the `ATTRIBUTION.md` row and the `LICENSE-translations.txt` paragraph, then run the tests and
   look at the result.

```sh
cargo test -p dalica-ui i18n                       # catalogs, placeholders, plurals, complete_menus coverage
CRAFT_FONTS_DIR=../craft-fonts cargo test -p dalica-ui i18n   # + glyph coverage for CJK scripts
DALICA_LANGUAGE=de dalica-cli snapshot --demo --script tour.jsonl -o de.png --size 1400x900
```

The tests fail on an empty entry, a placeholder present in one column only, a malformed plural
line, a duplicate key, or (for `complete_menus` languages) a missing enforced key. Missing entries
in other languages are reported, not failed. Render representative views and dialogs (library,
edit view, Folder Manager, Options, Export, About) at 1400 × 900 and 900 × 600 and look for
missing glyphs and truncated labels.

## Per-language style guides

Each language gets `docs/localization-<code>.md`, written in that language or in English, with:

- **Terminology:** a table of the recurring terms (Photo Tray, Hold, Auto Fix, Folder Manager,
  album, star, caption, tag, People, Places, light table) and the chosen translation, so later
  contributors stay consistent. Descriptive names of our own (e.g. **Auto Fix**) are translated by
  meaning, never by looking up what the reference product called the feature in that language.
- **Conventions:** formal or informal address, capitalisation of menu items, quotation marks,
  date patterns (e.g. weekday, day, month, year order for folder headers), number and file-size
  formatting.
- **Coverage:** what the catalog covers and what stays in English, and how it was checked (test
  counts, snapshots looked at), with the revision it was checked on.

## Release notes

Help ▸ Release Notes shows [`whats-new.md`](whats-new.md). Its lines (a heading, or one bullet
without the `- `) may be looked up like any message, so a catalog can translate the notes line by
line. An edited line no longer matches its translation and shows in English until the catalog is
updated; untranslated release-note lines are reported, never failed.
