# Localisation

Adapted from PhotoCraft `docs/localization.md`, `docs/localization-ko.md` and the
`crates/ui-egui/src/i18n/` catalog format (revision 59684266), and LightCraft
`docs/localization.md` (revision 88ea1a8); MIT OR Apache-2.0.

> Status: **The core is built (M15.7, parity row `SYS-I18N`); the catalogs are not.** `crates/ui/src/i18n/`
> has `tl!` and `tln!`, the catalog reader, the plural rules, the language registry (English
> only), System following the OS's languages, `DALICA_LANGUAGE`, Options ▸ General ▸ Language
> switching live, the number, size and date formats, the pseudo-locale `qps` and `cargo xtask i18n`.
> The strings are wrapped by area in M15.31–M15.33; the first catalogs (English and Thai for 1.0,
> if Thai can be done) come in M15.35.

Dalica follows the PhotoCraft i18n design (prior art only; our own code, see
[`architecture.md`](architecture.md) §9): **English source strings are the lookup keys**, UTF-8
**TSV catalogs** supply translations per language, and a missing entry falls back to English.

## Writing translatable code

- Wrap every user-facing literal: `tl!("Folder Manager")`; a word that reads differently by use
  takes a context: `tl!(ctx = "verb", "Open")`. Both give a `String`. Command labels in the
  registry are English and are drawn through `i18n::command_label(id, label)` (the command id's
  entry wins, then the label's); an Options row's label and hint through `i18n::option_label(key,
  label)` and `i18n::option_hint(key, hint)` (the `option:<key>` entry wins, then the English).
- Text with values uses named placeholders: `tl!("{count} photos selected", count = n)`. Never build
  sentences by concatenating translated fragments; word order differs between languages.
- Count-sensitive text uses the plural form, `tln!(n, "{n} photo", "{n} photos")` (`@plural` in the
  catalog, below; `{n}` is the count written in the language's digits, more placeholders may
  follow), never `"photo(s)"` or an English `"s"` suffix.
- Dates and numbers (folder header dates, file sizes, capture times) go through `i18n::format`
  (`number`, `decimal`, `file_size`, `month_name`, `weekday_name`, `long_date`,
  `long_date_with_weekday`), never hand-written `format!` patterns. The separators come from the
  language's registry row; the words and their order from the catalog (contexts `month`,
  `weekday` and `date`: `{month} {day}, {year}`). A form row's label is `{label}:` in the context
  `form label`, so a language may write its own colon.
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
| `@id` | a command id (e.g. `photo.star.toggle`): wins over the English text for that menu item, so one id can read differently from its twin with the same English label; `option:<key>` and `option:<key>/hint` do the same for an Options row |
| `@plural` | `source` is `"one\|other"` (the English forms); the translation lists the language's forms separated by `\|`, in the order of that language's plural rule |
| anything else | a disambiguating context passed to `tl!(ctx = "…", "…")` |

- Escapes `\n`, `\t`, `\\` inside a column. `{name}` placeholders must appear in both columns.
- A line that breaks a rule (not three columns, another backslash sequence, an empty column, a
  placeholder in one column only, a plural with the wrong number of forms, a key given twice, over
  64 KiB) is left out when the catalog is read, so that text shows in English, and one log line
  names the lines; reading a catalog never fails or panics. `cargo xtask i18n` fails on each.
- Lines starting with `#` and blank lines are ignored. Each file starts with a header comment that
  repeats these rules and the clean-room statement (below).
- Keep the trailing `…` of commands that open a dialog, and keep product and technology names
  (Dalica, JPEG, EXIF, XMP, GPS) in Latin letters.

## Languages and plural rules

Each language is registered in `i18n::LANGUAGES` (`crates/ui/src/i18n/languages.rs`) with its BCP-47
code (also the value stored in settings: `en`, `th`, `zh-Hans`, `pt-BR`), its **endonym** (shown in
the Language list, never translated), its script as an ISO 15924 code (`Latn`, `Thai`, `Hans`,
`Hant`, `Jpan`, …; it decides the fallback faces, and no script set is assumed), its plural rule,
its `complete_menus` flag, its catalog, and its decimal mark and thousands separator:

| Plural rule (`i18n::Plural`) | Forms, in catalog order | Languages (examples) |
|---|---|---|
| `Single` | other | Chinese, Japanese, Korean, Indonesian, Thai, Vietnamese |
| `OneOther` | one (1) / other | English, German, Spanish, Italian, Dutch, Greek |
| `ZeroOneOther` | one (0 and 1) / other | French, Brazilian Portuguese |
| `EastSlavic` | one (…1, not …11) / few (…2–…4, not …12–…14) / many | Russian, Ukrainian |
| `Polish` | one (1 only) / few (…2–…4, not …12–…14) / many | Polish |
| `Czech` | one (1) / few (2–4) / other | Czech, Slovak |

The registry holds English until the first catalogs land: the maintainer chose English and Thai
for 1.0, if Thai can be done (M14.R, 2026-10-19; M15.35).

## Choosing the language

Tools ▸ Options ▸ General ▸ Language ([`menus-and-shortcuts.md`](menus-and-shortcuts.md) §5)
switches without restarting: the rest of the interface follows on its next draw, and open views,
the selection and the undo history are kept. The list is System, then each registered language by
its endonym; the `ui` registry is handed to the engine at start (`Session::set_languages`), so it
is the one list. The default is **System** (stored as `system`), which follows the system's
preferred UI languages (read once, with no subprocess: the OS's own list on Windows and macOS, the
`LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG` variables elsewhere) and falls back to English; a
manual choice is saved and wins on later launches. Regional tags resolve to the closest
registered catalog: the exact code, then a shorter form (`de-AT` → `de`), then the same language
in another region (`pt-PT` → `pt-BR`); `C` and `POSIX` are English. Traditional and Simplified
Chinese stay distinct: `zh-TW`, `zh-HK`, `zh-MO` and `zh-Hant-*` find only Traditional, every other
`zh` only Simplified. `DALICA_LANGUAGE=<code>` overrides the setting for one launch (tests,
snapshots). Agents set the language with `options.set {values: {"interface.language": "<code>"}}`
over the control channel or MCP.

## Checking a translation: the pseudo-locale

`qps` is a language with no catalog: every text looked up is shown as its English in `⁅…⁆`, 40 %
longer (padded with `·`), built at run time from whatever key is asked for, placeholders kept, so
`{count} photos` reads `⁅3 photos····⁆`. It shows at a glance which texts bypass the catalogs and
whether a longer language still fits. Choose it with `options.set` (or `DALICA_LANGUAGE=qps`); the
Language list shows it only while it is the language chosen. The headless driver keeps the texts
each frame drew (`Driver::record_texts`, `Driver::drawn_texts`) and `i18n::pseudo::untranslated`
names those without the marks: under `qps` every text a frame draws is bracketed except user data
(file and folder names, captions, tags, …) and the languages' endonyms.

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
cargo xtask i18n                                   # the key set, each language's coverage, catalog faults
cargo test -p dalica-ui --test all i18n::          # the core: catalogs, plurals, tags, the pseudo-locale
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
without the `- `) may be looked up like any message (`i18n::release_note`), so a catalog can
translate the notes line by line. An edited line no longer matches its translation and shows in English until the catalog is
updated; untranslated release-note lines are reported, never failed.
