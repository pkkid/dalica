# README showcase screenshots

Adapted from LightCraft `docs/showcase/README.md` (revision 88ea1a8) and PhotoCraft
`docs/images/SOURCES.md` (revision 59684266); MIT OR Apache-2.0.

> **Status: the tools exist; the showcase does not yet.** `dalica-cli snapshot` (headless) and the
> control channel can make the shots of every feature built so far. No `docs/showcase/` scripts,
> `docs/images/` or `cargo xtask showcase` exist yet: the website's 0.2 screenshots were made by
> hand with the same commands (`website/README.md` → Images).

The screenshots in the README and in `docs/images/` are **produced by scripts**, never captured by
hand: each one is a JSON-lines script of `dalica-cli snapshot` steps
([`control-protocol.md`](control-protocol.md) §8) that sets up the UI, and the snapshot of its last
frame is the shot. Anyone can re-run them after a UI change and get the same picture, and reviewers
can see exactly what a screenshot shows. Image rules are in [`assets.md`](assets.md) §6; this file
is the procedure.

## 1. Layout

```text
docs/showcase/
  demo-library.jsonl      # one snapshot script per shot (a script makes one PNG)
  demo-edit.jsonl
  pd-library.jsonl        # shots using the public-domain set
  pd-edit.jsonl
  pd-setup.jsonl          # `dalica-cli run` script for the public-domain set: captions, tags, stars, edits
docs/images/
  library.jpg  edit-tuning.jpg  effects.jpg  tray.jpg  search.jpg  small-window.jpg  …
  SOURCES.md              # every image: what it shows, the photos in it, their sources and licences
```

## 2. Photos: procedural or public domain only

Only two sources of photos may appear in a showcase screenshot:

1. **The procedural demo library** (`--demo`, `dalica-scenes`): landscapes, gradients and patterns
   generated at run time, with synthetic dates and GPS. Original work, recorded once in
   `ATTRIBUTION.md`.
2. **The public-domain set**: real photographs that are **CC0 1.0 or public domain** with a stated
   basis — works of U.S. federal agencies (NASA, NOAA, the National Park Service, the U.S. Fish and
   Wildlife Service), photographs whose copyright has expired, or files their author dedicated to the
   public domain under CC0 (for example on Wikimedia Commons, or in the Smithsonian Open Access CC0
   collection). They are fetched by `cargo xtask corpus` into the gitignored `corpus/showcase/`,
   pinned by URL and sha256, with source and licence in the pin list (planned addition to the
   corpus table in `development.md`).

Never: personal photos; photos of identifiable private individuals; CC-BY, "no known restrictions"
or other attribution-only or unclear licences; trademarks or logos as the subject; anything from
`notes/`; anything showing the reference organizer or its assets. **People features** (face
boxes, the People group and pane) are shown only with the procedural faces or with public-domain
portraits of public figures (`assets.md` §6), and names shown are either the public figure's
documented name or obviously fictional names on procedural faces — never a fictional name on a real
person's face.

Folder names, album names, captions and tags in the showcase are ours (descriptive or fictional:
"Coastline", "Night sky", "Garden"); captions of public-domain photos state the real subject, author
and year when known.

## 3. Producing the shots

The default is **headless**: no window, no GPU, deterministic, works over SSH and in CI. Each
`snapshot` run opens one library, plays one script and writes one PNG; `--size` is in pixels and
`--scale` in pixels per point, so a sharp 1400 × 900-point shot is `--size 2800x1800 --scale 2`.

```sh
cargo build --release -p dalica-cli
# procedural demo library: --demo-fresh, so the script starts from the same state every run
target/release/dalica-cli snapshot --demo-fresh --script docs/showcase/demo-library.jsonl \
    -o docs/images/library.png --size 1400x900
# public-domain set (fetch it first: cargo xtask corpus --all): build a scratch library once,
# then shoot from it. Never point --library at a real library: snapshot writes to it.
LIB="$(mktemp -d)/library"
target/release/dalica-cli run --library "$LIB" --cmd folders.add \
    --params "{\"path\": \"$PWD/corpus/showcase\"}"
target/release/dalica-cli run --library "$LIB" docs/showcase/pd-setup.jsonl
target/release/dalica-cli snapshot --library "$LIB" --script docs/showcase/pd-edit.jsonl \
    -o docs/images/edit-tuning.png --size 2800x1800 --scale 2
# all scripts, then convert to JPEG and check sizes (planned)
cargo xtask showcase [NAME…]
```

`folders.add` takes an absolute path, which is why it is given on the command line rather than in
a checked-in script. `run` waits for the scan before its next line and before it exits.

`cargo xtask showcase` (planned; to be added to `development.md` §3 when it lands) runs every script in
`docs/showcase/`, encodes each PNG as JPEG quality 85 with the pure-Rust encoder (the README images
are JPEG; UI screenshots with flat greys compress well), fails if one is larger than 400 KB, and
fails if any script request failed (non-zero `snapshot` exit status). It never touches
`docs/images/SOURCES.md` or `ATTRIBUTION.md`: those are edited by hand in the same commit.

**Windowed variant**, to check that the window and the headless renderer agree, or for anything
headless cannot show yet (hover, several shots from one session). The window is driven with
control requests (`{"method": …, "params": …}` lines, `control-protocol.md` §6), not snapshot
steps, so it needs its own script; `dalica-cli` cannot send them yet (`run --connect` is planned,
`control-protocol.md` §9.1), so use a small client such as the one in §9.1:

```sh
cargo run --release -p dalica -- --control 18042 --demo-fresh \
    --automation-write-root "$PWD/docs/images" &
python3 dalica_ctl.py 18042 docs/showcase/demo-library.window.jsonl
```

`ui.screenshot` paths are absolute and must lie inside an `--automation-write-root`
(`control-protocol.md` §5.7). A `snapshot` writes its `-o` file wherever it is told, replacing a
file there. Never use OS screenshot tools or OS-level synthetic input to stage a shot (`AGENTS.md`
§3, §5): if a state cannot be reached through a script step or the control channel, add the missing
command or widget id.

## 4. Writing a script

A snapshot script has one step per line (`control-protocol.md` §8): `{"click": widgetId}`,
`{"key": "Ctrl+8"}`, `{"command": id, "params": {...}}` (an engine or UI command, run as a menu
item would) or `{"settle": true}`; blank lines and `#` lines are comments.

- Pick the size on the command line: 1400 × 900 is the default hero size; run the same script
  again at 900 × 600 for the minimum window when the layout matters.
- Put the library in a known state with commands (stars, captions, tags, edits), then reach the
  view **the way a user would** where a widget exists (`click` on tree rows, buttons and toggles,
  `key` for shortcuts), so the shot also exercises the UI path. Steps a click cannot express go
  through the UI commands: `select.set` (with `"mode": "toggle"` for a Ctrl-click), `view.edit`,
  `edit.tab`, `edit.tool.open`.
- Use deterministic ids (`control-protocol.md` §5.4; `--demo-fresh` gives the same ids every run)
  and never depend on timing: `snapshot` settles before the first step and after the last, and a
  `{"settle": true}` line waits for background work in between. No `sleep` lines.
- A failing step (an unknown widget, a refused command) stops the script with exit status 1 and
  writes no PNG; a line that is not a JSON object exits 2 before anything runs.
- Keep each script short and commented, one shot per script.

Example (`docs/showcase/demo-library.jsonl`, run with `--demo-fresh` at 1400x900 and at 900x600):

```jsonl
# Hero: the library view with a held tray, demo library
{"command": "photo.star.toggle", "params": {"ids": [2, 5, 9]}}
{"command": "photo.caption.set", "params": {"ids": [5], "caption": "Low tide, late afternoon"}}
{"click": "tree:folder:2"}
{"command": "select.set", "params": {"ids": [24]}}
{"command": "select.set", "params": {"ids": [26], "mode": "toggle"}}
{"click": "button:tray.hold"}
{"click": "toggle:pane.tags"}
{"settle": true}
```

An album joins the hero script when albums land (M9).

## 5. Planned shots

| Image | Shows | Source | From |
|---|---|---|---|
| `library.jpg` (hero) | light table with several sections, tree with albums and year dividers, a held tray, Tags pane | demo | M4 (Tags pane M6, albums M9) |
| `small-window.jpg` | the same at 900 × 600 (overflowing action buttons) | demo | M3 |
| `edit-tuning.jpg` | edit view, Tuning tab, histogram, filmstrip | public domain | M7 |
| `crop.jpg` | crop tool with ratio guides | public domain | M7 |
| `effects.jpg` | an effects tab with tile previews, one effect panel open | public domain | M12 |
| `search.jpg` | search mode banner and the search-results album | public domain | M9 |
| `people.jpg` | People group and pane | procedural faces / public-domain portraits of public figures | M13 |
| `places.jpg` | Places pane with geotagged photos (map tiles only from a provider whose terms allow it, with attribution visible) | public domain | M13 |

## 6. Before committing screenshots

1. **Look at every image** at full size: compare with [`ui-layout.md`](ui-layout.md) and
   [`design-system.md`](design-system.md) (and, if you have them, the reference screenshots in
   `notes/` — for layout only, never colours). A screenshot showing a bug is a bug report, not a
   showcase.
2. Check that nothing personal is visible: no home-directory paths with real user names (the demo
   and corpus paths are temporary or repository-relative), no real names other than documented
   public figures, no notification or system UI.
3. Add or update the row in `docs/images/SOURCES.md` (file, what it shows, each photo with source
   URL, author, year and licence basis; "UI: this project") and in `ATTRIBUTION.md`;
   `cargo xtask assets` enforces it.
4. Commit the script change and the images together; UI pull requests that change a showcase shot
   include the before/after pair (`process.md` §6).
