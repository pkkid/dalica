# Thai text: shaping and line breaking

The record of M15.47's spike: whether Dalica's own text path can show Thai well enough for Thai to
ship with 1.0 (the maintainer's answer at M14.R, 2026-10-19: English and Thai, "if Thai can be
done"). M15.35 reads the verdict.

## Verdict

**Thai passes.** Both parts are proven pure-Rust in our text path:

1. **Shaping passes.** egui 0.36's `epaint` shapes every run with `harfrust` (the pure-Rust port
   of HarfBuzz), over the faces `ui-kit` installs. With craft-fonts' Noto Sans Thai, the marks of
   the test words sit within **0.49 px** of a reference shaper at 16 px (the bar was 1 px).
2. **Line breaking passes**, through `ui_kit::linebreak::mark`: ICU4X's dictionary finds the word
   boundaries, a break mark that egui's wrapping honours is placed at each, and the fonts draw the
   mark with no width. A label wrapped at 200 px breaks only between words.

So M15.35 ships the Thai catalog. The known limits below are not reasons to hold Thai back; the
first is a trade made here, the others are for M15.35 and the review's hand checks.

## What was measured

Everything below is in `crates/ui-kit/tests/thai_text.rs`; the font-dependent checks run when the
build has `CRAFT_FONTS_DIR` (as the CI and release builds do) and are named as skipped otherwise.

- **Versions:** egui and epaint 0.36.2, `harfrust` 0.12.0; craft-fonts at pin `3d9ab3a`, its
  `fonts/noto-sans-thai/NotoSansThai.ttf` (OFL-1.1, sha256 `5a1c559b…`, installed by M15.8 and
  first for the Thai script); `icu_segmenter` 2.3.0 with its baked data (Unicode-3.0, on the
  allow-list of `cargo xtask deps`).
- **Glyphs:** every Thai consonant, vowel, tone mark, digit and sign, and every character of the
  test words and paragraph, is drawn by Noto Sans Thai in the Regular and SemiBold families (the
  baht sign ฿ by Inter, which has it).

### Shaping

The reference is HarfBuzz itself, outside the product and never shipped:
`hb-shape --show-extents NotoSansThai.ttf <word>` (HarfBuzz 14.1.0), recorded glyph by glyph in
the test. For each glyph the test compares the centre of the ink box egui draws with the centre
the reference gives, at 16 px:

| Word | What it checks | Furthest glyph |
|---|---|---|
| ที่ | tone mark ไม้เอก stacked over the upper vowel สระอี (the small form) | ≤ 0.49 px |
| กี้ | ไม้โท over สระอี | ≤ 0.49 px |
| ปู่ | lower vowel สระอู and a tone mark on the tall consonant ป (the narrow form, moved left) | ≤ 0.49 px |
| น้ำ | สระอำ split into นิคหิต and สระอา around the tone mark | ≤ 0.49 px |
| ฝั่ง | ไม้หันอากาศ and ไม้เอก moved aside from the tall consonant ฝ | ≤ 0.49 px |
| ฟ้า | ไม้โท moved aside from the tall consonant ฟ | ≤ 0.49 px |

egui yields as many glyphs as the reference, in the same order and with ink boxes of the same size
(the font's small and narrow mark forms are used; สระอำ becomes two glyphs), and the rows fit the stacked marks at 13 px. The fingerprint
`crates/ui-kit/tests/fingerprints/thai-text.json` holds the set at 40 px and 16 px, every
character, and a wrapped paragraph.

### Line breaking

Thai writes no spaces between words (a space ends a phrase or sentence), so a wrapped label must
find the word boundaries from a dictionary.

**egui's wrapping first.** `epaint` breaks a row at whitespace (`char::is_whitespace`), at or
before a Chinese or Japanese character, then at a dash or punctuation, and only then anywhere. It
never splits a cluster (a mark stays with its consonant), but in Thai it breaks mid-word. It does
**not** break at U+200B ZERO WIDTH SPACE, the usual mark: that is not whitespace to Rust (test
`egui_does_not_break_at_a_zero_width_space`). It does break at U+2009 THIN SPACE, and a font tweak
(`thin_space_width`) sets how wide egui draws a thin space.

**So the mark is U+2009 at zero width.** `ui_kit::linebreak::MARK` is U+2009, and `fonts.rs` gives
every face `thin_space_width: 0`. `linebreak::mark(&str) -> Cow<str>` places a mark at each
boundary that ICU4X's `LineSegmenter` (dictionary mode) finds between two Thai characters, and
never before ฯ, ๆ, a following vowel or a mark, nor after a leading vowel (เ แ โ ใ ไ). Text
without Thai comes back borrowed, unchanged (Latin, CJK and Cyrillic checked), and marking marked
text changes nothing. On a 73-word paragraph of seven plain sentences the marks fall exactly
between the words, and a label wrapped at 200 px breaks only at them; the marked and the plain
paragraph are equally wide.

**At run time, not in the catalog.** The other candidate was to write the marks into the Thai
catalog at build time (`cargo xtask i18n`), which costs the product nothing. It was dropped
because captions, tags and folder names typed in Thai must wrap the same way, and only a run-time
segmenter sees them. The cost of the run-time one:

| Cost | Measured |
|---|---|
| Binary size | +1.77 MiB (a stripped release probe with and without the segmenter: 2,197,120 vs 345,728 bytes). The baked data carries the Thai, Lao, Khmer and Burmese dictionaries in one table; a Thai-only data build could cut it later. |
| Start-up | none: the segmenter is made on the first `mark` (about 1–3 µs; the data is baked, nothing is parsed). |
| `mark` on the release notes' length (52,205 Thai characters, 13,002 marks) | 1.6 ms in a release build (48 ms in the test's debug build) |
| `mark` on a 29-character caption | about 3 µs in a release build (25 µs in debug) |

**Frame time:** mark text once, where it is loaded or changes (a catalog at load, a caption per
revision), never per frame.

## How M15.35 and later work use it

- Mark the Thai catalog's text once at load with `linebreak::mark` (English needs none).
- Mark user data only for display, where a view wraps it (captions, tags, folder names), cached
  with the text's revision. Never mark the value of a text field, or text that is saved, compared,
  searched or sent: the marks are for wrapping only.
- `crates/ui-kit/tests/thai_text.rs` is the pattern for Thai checks in `crates/ui`: the glyph and
  mark checks with the fonts, the wrap check without them.

## Known limits

- **Thin spaces draw with no width, in every language.** U+2009 (and U+202F NARROW NO-BREAK SPACE,
  which egui treats alike) typed into a caption or a file name now shows as no space at all; a
  row may still break there. The alternative, a tweak only under the Thai interface, would show
  marked Thai captions with gaps between words under any other language. Rare in user text; the
  trade is reversed by dropping the tweak in `fonts.rs` once egui breaks at U+200B.
- **If egui learns to break at U+200B**, `egui_does_not_break_at_a_zero_width_space` fails on the
  upgrade: then make `MARK` U+200B and drop the `thin_space_width` tweak.
- **The text cursor after สระอำ.** egui counts a row's characters by its glyphs, and สระอำ shapes to
  two, so in a Thai text field the caret is drawn one glyph off after each ำ (น้ำ, ทำ, กำลัง), and a
  click there lands one character off. Typing, deleting and the arrow keys work on the right
  characters, and nothing panics; display is unaffected. An upstream egui issue, for the review's
  input-method pass by hand.
- **The dictionary can misread an ambiguous run.** ICU4X splits ที่นอน (a mattress) as ที่ + นอน
  when the context is unclear; a break there falls between two real words, so the worst case is a
  slightly odd line end.
