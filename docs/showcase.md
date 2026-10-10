# README showcase screenshots

Adapted from LightCraft `docs/showcase/README.md` (revision 88ea1a8) and PhotoCraft
`docs/images/SOURCES.md` (revision 59684266); MIT OR Apache-2.0.

> **Status: Planned (M2).** The mechanism (headless snapshots and the control channel) lands in M2;
> the first real showcase shots come with M4 (library view) and M7 (edit view). Nothing exists yet.

The screenshots in the README and in `docs/images/` are **produced by scripts**, never captured by
hand: each one is a JSON-lines script of control-protocol requests
([`control-protocol.md`](control-protocol.md)) that sets up the UI and takes a screenshot. Anyone can
re-run them after a UI change and get the same picture, and reviewers can see exactly what a
screenshot shows. Image rules are in [`assets.md`](assets.md) §6; this file is the procedure.

## 1. Layout

```text
docs/showcase/
  demo-library.jsonl      # one script per shot (or per related group of shots)
  demo-edit.jsonl
  pd-library.jsonl        # shots using the public-domain set
  pd-edit.jsonl
  pd-setup.jsonl          # shared setup for the public-domain set: captions, tags, stars, albums
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

The default is **headless** — no window, no GPU, deterministic, works over SSH and in CI:

```sh
cargo build --release -p dalica-cli
# procedural demo library
target/release/dalica-cli snapshot --demo --script docs/showcase/demo-library.jsonl \
    -o docs/images/library.png --size 1400x900
# public-domain set (fetch it first: cargo xtask corpus --all)
target/release/dalica-cli snapshot corpus/showcase --script docs/showcase/pd-edit.jsonl \
    -o docs/images/edit-tuning.png --size 1400x900 --scale 2
# all scripts, then convert to JPEG and check sizes (planned)
cargo xtask showcase [NAME…]
```

`cargo xtask showcase` (planned; to be added to `development.md` §3 when it lands) runs every script in
`docs/showcase/`, encodes each PNG as JPEG quality 85 with the pure-Rust encoder (the README images
are JPEG; UI screenshots with flat greys compress well), fails if one is larger than 400 KB, and
fails if any script request failed (non-zero `snapshot` exit status). It never touches
`docs/images/SOURCES.md` or `ATTRIBUTION.md`: those are edited by hand in the same commit.

**Windowed variant**, to check that the window and the headless renderer agree, or for anything
headless cannot show yet:

```sh
cargo run --release -p dalica -- --control 18042 --demo &
target/release/dalica-cli run --connect 127.0.0.1:18042 --script docs/showcase/demo-library.jsonl
```

Screenshot paths in the scripts are relative to the repository root; run the commands from there.
`snapshot` treats its working directory as a write root; for the windowed run, start the app with
`--automation-write-root "$PWD/docs/images"` (`control-protocol.md` §5.7, §8). Never use OS screenshot tools or OS-level synthetic input to stage a
shot (`AGENTS.md` §5): if a state cannot be reached through the control channel, add the missing
method or widget id.

## 4. Writing a script

- Start with `ui.resize` (1400 × 900 is the default hero size; add a 900 × 600 shot for the
  minimum window when the layout matters).
- Put the library in a known state with `engine.execute` (stars, captions, tags, albums, edits),
  then reach the view **the way a user would** — `ui.clickWidget` on tree rows and thumbnails,
  `ui.key`, `ui.menu.invoke` — so the screenshot also exercises the UI path.
- Use deterministic ids (`control-protocol.md` §5.4) and never depend on timing: `ui.settle` before
  every `ui.screenshot`, `ui.wait` only for things that are time-based by design (tooltips). No
  `sleep` lines.
- Hover states, open menus and tooltips are fine when they explain a feature; a stray hover
  highlight is not — end with the pointer over empty space (`ui.pointer` move) before the shot.
- Keep each script short and commented; one shot per `ui.screenshot`, with a `path` naming the
  output.

Example (`docs/showcase/demo-library.jsonl`):

```jsonl
# Hero: the library view with a held tray, demo library, 1400 x 900
{"method": "ui.resize", "params": {"width": 1400, "height": 900}}
{"command": "photo.star.toggle", "params": {"ids": [2, 5, 9]}}
{"command": "album.new", "params": {"name": "Best of the coast", "ids": [2, 5, 9, 14]}}
{"command": "photo.caption.set", "params": {"ids": [5], "caption": "Low tide, late afternoon"}}
{"method": "ui.clickWidget", "params": {"id": "tree:folder:2"}}
{"method": "ui.clickWidget", "params": {"id": "thumb:14"}}
{"method": "ui.clickWidget", "params": {"id": "thumb:16", "ctrl": true}}
{"method": "ui.clickWidget", "params": {"id": "button:tray.hold"}}
{"method": "ui.clickWidget", "params": {"id": "toggle:pane.tags"}}
{"method": "ui.pointer", "params": {"events": [{"kind": "move", "x": 700, "y": 880}]}}
{"method": "ui.settle"}
{"method": "ui.screenshot", "params": {"path": "docs/images/library.png"}}
# The same at the minimum window size
{"method": "ui.resize", "params": {"width": 900, "height": 600}}
{"method": "ui.settle"}
{"method": "ui.screenshot", "params": {"path": "docs/images/small-window.png"}}
```

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
