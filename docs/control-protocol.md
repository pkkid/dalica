# Control protocol

Adapted from LightCraft `docs/control-protocol.md` (revision 88ea1a8) and PhotoCraft
`docs/control-protocol.md` (revision 59684266); MIT OR Apache-2.0.

> **Status: Planned (M2; menu methods M3).** Nothing here exists yet. The control channel, widget
> registry and `ui.screenshot` are milestone **M2**; the headless renderer and
> `dalica-cli snapshot` are **M1** (see [`roadmap.md`](roadmap.md)). Methods and widget ids that
> need a later region (edit view M7, panes M6, effects M12, people M13) say so; until then
> they answer with the `unsupported` error.
> Keep this file in sync with `crates/ui/src/control/` (the method table is in its `mod.rs`).

The control channel lets an agent, a test or a script **drive and see** the real Dalica UI: run any
command by id, read the library and UI state, click widgets by automation id, press keys, take
screenshots. It is how agents verify their work (`AGENTS.md` §5), how README screenshots are made
([`showcase.md`](showcase.md)), and the transport under MCP connect mode ([`mcp.md`](mcp.md)).

Read with: [`architecture.md`](architecture.md) §3 (commands) and §7 (automation),
[`menus-and-shortcuts.md`](menus-and-shortcuts.md) (command ids),
[`ui-layout.md`](ui-layout.md) (regions), [`data-model.md`](data-model.md) (what commands write).

Implementation (M2): `apps/dalica/src/control/` (TCP transport, token file, auth, limits, the window
and `--headless` hosts) and `crates/ui/src/control/` (method handlers, shared by the window and the
headless driver).

## 1. Starting the server

```sh
dalica --control 7990 --demo                         # the demo library (persistent, in the temp folder)
dalica --control 18042 --control-token-file ~/.cache/dalica/agent-a.token \
       --automation-read-root "$PWD/corpus" --automation-write-root "$PWD/target/shots"
```

| Flag | Env equivalent | Meaning |
|---|---|---|
| `--control [PORT]` | `DALICA_CONTROL_PORT` | Listen on `127.0.0.1:PORT` (default **7990**). Loopback only; there is no flag to bind another address. |
| `--control-token-file PATH` | `DALICA_CONTROL_TOKEN_FILE` | Read the token from `PATH`; if the file does not exist, create it with a fresh token (§2.1). |
| `--automation-read-root DIR` (repeatable) | `DALICA_AUTOMATION_READ_ROOT` (`:`/`;`-separated) | Folders automation may add to the library or read from (§5.7). |
| `--automation-write-root DIR` (repeatable) | `DALICA_AUTOMATION_WRITE_ROOT` | Folders automation may write new files into (§5.7). |
| `--automation-allow-external` | — | Allow commands with effects outside Dalica (§5.6). Off by default. |
| `--demo` | — | Procedural demo library (`dalica-scenes`) in `dalica-demo` in the OS temporary folder: made on first use and **reused** by later runs (so a star survives a restart; delete the folder to start over), with **isolated settings**: the library index, `folders.json` and `settings.json` live in the demo folder, so nothing in the user's settings, library index or window state is read or changed. Ids are deterministic (§5.4). |
| `--demo-fresh` | — | Like `--demo`, but in a new folder `dalica-demo-<pid>-<n>` in the OS temporary folder (private to the user on Unix), removed with everything in it when the app or the CLI command exits: a clean slate for every automation run. A killed process leaves the folder behind. |

- If the port is already in use, Dalica prints the reason to standard error and **exits with status
  2** instead of starting without a control channel, so a script can never end up driving another
  instance that happens to own the port.
- **Parallel agents** use their own port in **18000–19999** (never 7990) and their own token file
  (`AGENTS.md` §7).
- The control channel is off unless `--control` is given. Do not enable it permanently.

## 2. Framing and authentication

Newline-delimited JSON (UTF-8, `\n`-terminated). One request per line, one reply per line, in
request order per connection.

### 2.1 The token

- `--control-token-file PATH`: an existing file is reused (its first line, 64 hex characters); a
  missing file is created with a fresh **256-bit** token from the OS random source, written
  atomically with mode **0600** on Unix (a user-only ACL on Windows). An existing file that is
  readable by group or others is refused at startup with a message saying how to fix it.
- Without the flag, Dalica creates a fresh token for this launch in the **default token file**
  `<run>/control-<port>.token`, where `<run>` is `$XDG_RUNTIME_DIR/dalica` on Linux when that
  variable is set and Dalica's cache directory + `/run` otherwise (Linux `$XDG_CACHE_HOME/dalica/run`,
  macOS `~/Library/Caches/Dalica/run`, Windows `%LOCALAPPDATA%\Dalica\cache\run`), mode 0600 (the
  directory 0700), and deletes it on exit.
  `dalica-cli` (`run --connect`, `mcp --connect`) reads this file for the port it connects to, so
  the token never appears on a command line. The log names the file, never the token.
- The token is a bearer credential: anyone who can read it can drive the app. Never commit, log,
  paste or pass it as a command-line argument.

### 2.2 The first line must be `auth`

```text
→ {"id": "auth", "method": "auth", "params": {"token": "<64 hex characters>"}}
← {"id": "auth", "ok": true, "result": {"app": "dalica", "version": "0.1.0 (dev build)", "protocol": 1, "pid": 41237}}
```

No other method is dispatched before `auth` succeeds. A wrong token (compared in constant time), a
missing `auth`, or any other first line gets one error reply (`code: "unauthorized"`) and the
connection is closed. `protocol` is an integer bumped on incompatible changes.

### 2.3 Requests and replies

```text
→ {"id": 1, "method": "engine.execute", "params": {"command": "photo.star.toggle", "params": {"ids": [42]}}}
← {"id": 1, "ok": true, "result": {"starred": [42]}}
→ {"id": 2, "method": "ui.clickWidget", "params": {"id": "button:nope"}}
← {"id": 2, "ok": false, "code": "not_found", "error": "no widget `button:nope` on screen (see ui.widgets)"}
```

- A request is a JSON **object** with a string `method`; `id` (string or number) is echoed back;
  `params` is an object (omitted or `null` means `{}`; any other type is `bad_params`).
- A reply has `ok`, then `result` (success) or `code` + `error` (failure). `error` is an English
  sentence an agent can act on; `code` is stable (§3).
- **Only requests are read.** A line that is not JSON, not an object, lacks `method`, is not valid
  UTF-8, is longer than the line limit or nests arrays and objects deeper than the depth limit (§4)
  gets one error reply (`code: "parse"`) and the server **closes the connection**; nothing sent
  after it on that connection runs. This keeps an HTTP
  request (for example a web page's cross-origin `fetch` to `127.0.0.1:7990`) from smuggling a
  command in its body: its request line is rejected first, and it could not authenticate anyway.
  Clients that see a `parse` error reconnect.
- Blank lines are skipped. Several connections may be open; requests from different connections
  interleave at frame boundaries, each connection's requests run in order.

## 3. Errors

| `code` | When | Connection |
|---|---|---|
| `parse` | the line is not a request (see §2.3), exceeds the line limit or nests deeper than the depth limit | closed |
| `unauthorized` | first line is not a successful `auth` | closed |
| `unknown_method` | no such method (`methods` lists them) | open |
| `unknown_command` | `engine.execute` / `ui.menu.invoke` with an unregistered id | open |
| `bad_params` | a missing, extra or ill-typed parameter; the message names it and the accepted keys | open |
| `disabled` | the command's `enabled` is false now; the message is the toast the UI would show ("Select some photos first") | open |
| `needs_confirm` | a destructive command without `"confirm": true` (§5.5); `prompt` holds the dialog text | open |
| `not_found` | an unknown photo, folder, album, widget, menu path or job id | open |
| `busy` | a running job owns what the command would change ("Export is still running on these photos") | open |
| `path_denied` | a path outside the automation roots, or a forbidden target (§5.7) | open |
| `external_denied` | a command with effects outside Dalica without `--automation-allow-external` (§5.6) | open |
| `unsupported` | the feature is not built yet (planned milestone named in the message) | open |
| `limit` | a size, count or duration over its limit (§4) | open |
| `timeout` | the UI did not answer within the reply deadline; the work may still have happened — inspect before retrying | open |
| `unsaved` | the change was applied (and is undoable) but could not be written to disk; it is retried (§5.8) | open |
| `internal` | an escaped panic caught by the dispatch guard; the app keeps running and the panic is logged | open |

## 4. Limits

| Limit | Value |
|---|---|
| Request line | 1 MiB (encoded, including the newline) |
| JSON nesting depth | 64 levels of arrays and objects, the request object included; deeper lines get `parse` and are closed (§2.3) |
| Reply line | 8 MiB; a larger result is replaced by a `limit` error (screenshots: write to a `path` instead) |
| Open connections | 16; further ones get one `limit` error line and are closed |
| Socket read/write timeout | 30 s idle |
| Reply deadline | 60 s per request; a request still queued at the deadline is rejected before dispatch (`timeout`). Long commands return `{job, pending: true}` before it instead (§5.3) |
| `batch` steps | 256 |
| `ui.clickWidget` `count` | 1–8 |
| Drag `steps` | 1–1,000 |
| `ui.pointer` events | 10,000 per request |
| `ui.type` text | 64 KiB |
| `ui.wait` / `ui.settle` timeouts | ≤ 60,000 ms |
| Screenshot size | ≥ 1 px and ≤ 16,384 px per edge, ≤ 64 Mpx in total, scale in (0, 4] |
| `library.inspect` photo listing | `limit` ≤ 1,000 per request (page with `offset`) |

Every limit is checked before anything changes; an over-limit request has no effect.

## 5. Execution model

### 5.1 Where requests run

Requests are answered on the UI thread between frames, by the same `App` code the mouse and
keyboard drive. An arriving request wakes the UI (egui repaints on demand), and requests are served
from the app's logic tick, which keeps running when the window is occluded, minimised or the display
sleeps. The handlers obey the UI-thread rules of `architecture.md` §5: a handler never decodes,
touches the file system or blocks; work is queued to workers and the reply waits for it as described
below.

### 5.2 Input replies after the input has played out

Input methods (`ui.clickWidget`, `ui.dragWidget`, `ui.hoverWidget`, `ui.pointer`, `ui.key`,
`ui.type`, `ui.scroll`, `ui.menu.invoke`) queue real egui events. Their reply is sent only **after
the events have been consumed by a frame and one more frame has been laid out**, so the next
request observes the effect: after a click that opens a context menu, `ui.widgets` lists the menu's
items; after `ui.key Ctrl+H`, `library.inspect` shows the held photos. This is identical in the
window and in the headless driver.

Input replies do not wait for work the input started (thumbnails, a scan, an export). Use
`ui.settle` (renders and animations) or `jobs.wait` (long jobs) for that.

### 5.3 Commands, undo and jobs

- `engine.execute` runs a command through `Session::execute` (or `App::run` for `UI_COMMANDS`): one
  call = **exactly one undo step**, never a dialog. Commands that need input take it as params
  (`photo.rename {ids, name}`); omitted required params are `bad_params`. For menu-click behaviour
  (open the command's dialog) use `ui.menu.invoke`.
- Commands that act on photos take `ids` (photo ids); without `ids` they act on the **Photo Tray**
  contents, exactly like the bottom-bar buttons (`ui-layout.md` §1.5).
- **Long commands** (scan/add folder, import, export, Save of many photos, batch edit, rebuild,
  duplicates, collage output, face detection) run as **jobs** on workers,
  as in the UI. By default the reply waits for the job and carries its result. With
  `"wait": false` the reply is `{job, pending: true}` at once. A job still running 55 s into the
  reply deadline replies `{job, pending: true}` too, so a long export never ends in a blind
  `timeout`. Follow up with `jobs.wait` / `jobs.list`.
- Input-driven changes behave exactly like the user's: a slider drag or a typing session in the
  caption field coalesces into one undo step.

### 5.4 Ids

- **PhotoId** and **FolderId** are integers assigned by the library index; they are stable for the
  life of the index and change only on a rebuild. Paths are the durable identity: every listing
  carries both. **AlbumId** and **PersonId** are the short hex strings stored in sidecars
  (`a91c0e`, `p7f3a2c`), stable across rebuilds.
- In the `--demo` and `--demo-fresh` libraries, ids are **deterministic**: folders are numbered
  from 1 in tree order (flat view, date sort: folder 1 is the newest, `2026-06-06 Garden`) and
  photos from 1 in light-table order (each folder's photos oldest first: photo 1 is
  `IMG_0041.jpg`), so scripts can name them, and two demo libraries list the same ids for the same
  paths under the demo folder. A persistent demo whose ids are out of order (made by an older
  version) has its index set aside as `library.unordered-<time>` and is indexed again on its next
  start; its stars come back from the sidecars.
- A **section id** names a light-table section: `folder:<FolderId>`, `album:<AlbumId>`,
  `album:starred`, `album:recent`, `album:search` (the temporary search-results album),
  `person:<PersonId>`, `person:unnamed`.

### 5.5 Destructive commands need `confirm: true`

Every command whose UI asks first (`data-model.md` §1.5) refuses over automation unless its params
contain `"confirm": true`: `photo.delete`, `folder.delete`, `album.delete`, `photo.revert`,
`edit.undoAll`, `photo.save` (the first-time explanation), clearing a tray with held items,
`folder.remove`, `library.rebuild`, and the import options that delete from a card. The error
(`needs_confirm`) carries `prompt`: the dialog's text and buttons, so an agent can show it to its
user. `ui.menu.invoke` and clicks open the real confirmation dialog instead. Deletes still go to the
OS trash; `confirm` never turns a trash delete into a permanent one.

### 5.6 Effects outside Dalica are refused by default

Automation must not reach beyond the app. Without `--automation-allow-external`, these refuse with
`external_denied`: Locate on Disk (opens the file manager), Open in External Editor, Check for
Updates, opening the help or release-notes web pages, and anything that would
use the network (map tiles, geocoding, model downloads; `AGENTS.md` "Privacy").

**Native file pickers are never opened by automation.** `engine.execute` takes paths as params. A
picker requested by synthetic input (clicking *Browse…*, *Add Folder…* through `ui.menu.invoke`) is
replaced by an in-app stub: `ui.inspect` → `filePicker: {command, kind: "folder" | "file" | "save",
title}`, answered with `ui.filePicker {path}` or `ui.filePicker {cancel: true}`. Pickers opened by
real user input stay native.

### 5.7 Paths

All path params (folder to add, export target, screenshot `path`, render `path`) are absolute,
normalised paths; `..` components are refused; symlinks are resolved before the check.

- **Reading** (adding a watched folder, `library.openFile`, import from a folder): the path must be
  inside an `--automation-read-root` or an already watched folder. With no read root, adding new
  folders over automation is refused (`path_denied`), except in `--demo`.
- **Writing new files** (export, collage, screenshots, renders): the target must be inside an `--automation-write-root`. With no write root
  given, the defaults are the OS temporary directory and `<cache>/Dalica/automation/`.
- Never inside a watched folder or any `.dalica-originals/` (a screenshot or export there would turn
  into a library photo or touch a backup), and never over an existing file, except that a screenshot
  or render may replace an existing `.png`. Exports never overwrite (collisions get `-001`, `data-model.md` §6).
- In-library file operations (Save with backup, Save a Copy, move/rename between watched folders,
  delete to trash) are ordinary commands with their usual safety and undo; they need no root, but
  the destructive ones need `confirm` (§5.5).

### 5.8 When a write fails

Commands apply to the in-memory library at once; the sidecar, metadata and index writes follow
(`data-model.md` §3, §5). If a write fails (disk full, volume gone, read-only folder), the command
replies `ok: false`, `code: "unsaved"`, with `"saved in memory but not written to disk: <reason>;
Dalica will retry"`. The change is applied, undoable and queued; the writer retries every few
seconds and on the next change, so nothing is lost once the disk is writable again — unless the app
quits first. Meanwhile `library.inspect` → `pendingWrites` is `{sidecars, metadata, index, error}`
and the status line shows a `caution` marker (widget `indicator:unsaved`).

### 5.9 When the library cannot be opened

If the library is locked by another Dalica process, unreadable, or from a newer version, the app
starts with an empty, temporary session (never with demo photos) and a dialog: `ui.inspect` →
`libraryProblem: {path, error, temporarySession}`; buttons `button:library.retry`,
`button:library.choose`, `button:library.temporary`, `button:library.quit`. A temporary session
writes nothing and shows `indicator:temporarySession`.

## 6. Methods

| Method | Params | Result |
|---|---|---|
| `auth` | `{token}` | `{app, version, protocol, pid}` (§2.2) |
| `methods` | — | every method with its params doc |
| `engine.execute` | `{command, params?, wait?}` | the command's result; `{job, pending: true}` for a job not yet done |
| `engine.commands` | `{filter?, enabledOnly?}` | the command registry (§6.2) |
| `batch` | `{steps, stopOnError?}` | `{completed, failed, results}` (§6.3) |
| `library.inspect` | `{photos?, section?, offset?, limit?}` | library, tree, sections, selection, tray, search (§6.4) |
| `ui.state` | — | the view state (§6.5) |
| `ui.set` | partial view state | the new view state |
| `ui.inspect` | `{resetPerf?}` | everything in `ui.state` plus regions, popups, dialogs, perf (§6.6) |
| `ui.resize` | `{width, height}` | `{width, height}` in points |
| `ui.widgets` | `{filter?, region?}` | `[{id, rect, enabled, checked?, label?, role}]` (§6.7) |
| `ui.clickWidget` | `{id, count?, button?, fx?, fy?, ctrl?, shift?, alt?, cmd?}` | `{rect}` |
| `ui.dragWidget` | `{id, toWidget? \| toX, toY \| dx, dy, steps?, fx?, fy?, ctrl?, …}` | `{from, to}` |
| `ui.hoverWidget` | `{id, fx?, fy?, ms?}` | `{rect}` |
| `ui.pointer` | `{events, space?, button?, ctrl?, shift?, alt?, cmd?}` | `{events}` (§6.8) |
| `ui.key` | `{key \| combo, state?, ctrl?, shift?, alt?, cmd?, macCtrl?}` | `{}` (§6.9) |
| `ui.type` | `{text}` | `{}` |
| `ui.scroll` | `{dx?, dy?, widget?, ctrl?, shift?, alt?}` | `{}` |
| `ui.wait` | `{ms}` | `{frames}` |
| `ui.menu.tree` | `{context?}` | the menu bar (or a context menu) as shown (§6.10) |
| `ui.menu.invoke` | `{path \| id, params?, wait?}` | the command's result, or `{dialog}` |
| `ui.dialog.confirm` / `ui.dialog.cancel` | `{button?}` | the dialog's command result / `{}` |
| `ui.filePicker` | `{path} \| {cancel: true}` | the command's result (§5.6) |
| `ui.screenshot` | `{path?, headless?, widget?, region?, scale?, settle?}` | `{path, width, height, headless}` (§6.11) |
| `ui.settle` | `{timeoutMs?, jobs?}` | `{settled, frames, ms, busy}` (§6.12) |
| `jobs.list` | — | `[{job, kind, label, state, progress, result?, error?}]` (§6.13) |
| `jobs.wait` | `{job, timeoutMs?}` | the job's final entry, or `{job, pending: true}` |
| `jobs.cancel` | `{job?}` | `{cancelled: [job…]}`; no `job` = every cancellable job |
| `app.quit` | — | `{}`; pending sidecar/metadata writes are flushed first |

There are no hidden aliases: a misspelt method is `unknown_method`.

### 6.1 `engine.execute`

```json
{"id": 3, "method": "engine.execute", "params": {"command": "photo.caption.set", "params": {"ids": [7], "caption": "Tram 28 at sunset"}}}
{"id": 4, "method": "engine.execute", "params": {"command": "output.export", "params": {"album": "a91c0e", "dir": "/tmp/out", "size": 1600}, "wait": false}}
```

Runs an engine command or a `UI_COMMANDS` entry (`view.edit`, `pane.tags`, `view.thumbsSmall`)
by id. `params` is the command's own JSON, documented by its `params` string in `engine.commands`.
Disabled commands fail with `disabled` and change nothing. Query commands (`photo.inspect`,
`library.query`, …) change nothing and record no undo step.

### 6.2 `engine.commands`

```json
[{"id": "photo.star.toggle", "label": "Star / Unstar", "menu": "Picture/Star / Unstar", "shortcut": "Ctrl+8",
  "params": "{\"ids\":[PhotoId]=tray, \"on\":bool=toggle}", "enabled": true, "ui": false, "checked": null,
  "live": true}]
```

`shortcut` is the effective binding (Cmd on macOS is written `Cmd`); `ui: true` marks `UI_COMMANDS`;
`checked` is set for checkable items; `live: false` marks menu-catalog items whose command is not
registered yet (they render disabled). `filter` matches id, label or menu path (case-insensitive);
`enabledOnly` drops disabled entries.

### 6.3 `batch`

```json
{"id": 9, "method": "batch", "params": {"stopOnError": true, "steps": [
  {"command": "select.all"},
  {"method": "ui.key", "params": {"combo": "Ctrl+H"}},
  {"method": "ui.clickWidget", "params": {"id": "tree:folder:3"}},
  {"method": "ui.screenshot", "params": {"path": "/tmp/held.png"}}
]}}
```

A step is `{command, params?, wait?}` (shorthand for `engine.execute`) or `{method, params?}`.
`batch` itself, `auth` and `app.quit` are not allowed as steps. Steps run in order with the same
semantics as separate requests (input plays out before the next step); each command step is its own
undo step — a batch is **not** a transaction. `stopOnError` defaults to `true`. The reply lists
every step's `{ok, result | code + error}` and stops early, with a `limit` error, when the aggregate
reply would exceed 8 MiB (the step that hit the budget may have run).

### 6.4 `library.inspect`

The library as the user sees it. Also an engine query command (`library.inspect`), so headless MCP
and the CLI return the same JSON; the control method adds the UI-only fields (`current`,
`visibleSections`, `scroll`).

```json
{
  "revision": 1842,
  "demo": true,
  "watched": [{"path": "/tmp/dalica-demo-41237/Pictures", "mode": "always", "online": true}],
  "counts": {"photos": 212, "videos": 6, "folders": 14, "albums": 3, "people": 0, "hidden": 4},
  "tree": {
    "mode": "flat", "sort": "date", "reverse": false,
    "groups": [
      {"group": "albums", "collapsed": false, "rows": [
        {"widget": "tree:album:starred", "section": "album:starred", "name": "Starred Photos", "count": 12},
        {"widget": "tree:album:a91c0e", "section": "album:a91c0e", "name": "Best of 2026", "count": 40}]},
      {"group": "folders", "collapsed": false, "rows": [
        {"widget": "tree:folder:1", "section": "folder:1", "name": "Lisbon Weekend", "count": 48,
         "path": "/tmp/dalica-demo-41237/Pictures/2026-06 Lisbon", "depth": 0, "year": 2026,
         "hidden": false, "offline": false}]}]
  },
  "sections": [{"section": "folder:1", "name": "Lisbon Weekend", "count": 48, "shown": 48,
                "date": "2026-06-04", "place": "Lisbon, Portugal", "description": "", "edited": 3}],
  "current": "folder:1",
  "visibleSections": ["folder:1", "folder:2"],
  "selection": {"section": "folder:1", "wholeSection": false, "ids": [3, 4], "anchor": 3, "focus": 4},
  "tray": {"token": null, "items": [3, 4, 57], "held": [57], "traySelected": []},
  "search": {"active": false, "text": "", "filters": {"starred": false, "faces": false, "videos": false,
             "geotagged": false}, "age": 0, "matches": null},
  "view": {"hiddenPictures": false, "smallPictures": false, "caption": "none", "thumbSize": 144},
  "undo": {"undo": "Star", "redo": null, "depth": 5},
  "unsavedEdits": {"photos": 3, "sections": ["folder:1"]},
  "pendingWrites": null,
  "problems": [],
  "scan": {"running": false, "foldersQueued": 0, "found": 218, "indexed": 218}
}
```

- `photos: true` (or a section id) adds `photos: [{id, section, path, name, star, caption, tags,
  hidden, rotate, edited, recipe, geo, date, width, height, bytes, video, held, selected}]` in
  light-table order, paged by `offset` / `limit` (default 200). `section` restricts the listing.
- `selection.wholeSection` is true when a tree click selected the section as a unit (the tray then
  shows a single token: `tray.token = {section, count}`).
- `problems` lists damaged sidecars kept aside (`.corrupt-<time>`), offline volumes and unreadable
  files, with paths.
- **People data is withheld by default:** person names, face rectangles and assignments appear only
  when the app was started with `--automation-allow-people`; otherwise people rows show
  `name: null` and counts. Face descriptors are never returned (`AGENTS.md` "Privacy").

### 6.5 `ui.state` and `ui.set`

The view state: what `UI_COMMANDS` change and what is persisted between sessions.

```json
{"view": "library", "room": "library", "rooms": ["library"], "treeMode": "flat", "treeWidth": 240,
 "pane": null, "paneLarge": false, "thumbSize": 144, "caption": "none", "fullScreen": false,
 "edit": {"photo": null, "tab": "basic", "tool": null, "compare": "one", "zoom": "fit", "panelVisible": true},
 "slideshow": null, "language": "en"}
```

`view` is `library | edit | slideshow | viewer`; `room` is the active tab (`library`,
`import`, `collage`). `ui.set` takes any subset of
these fields, validates all of them first (unknown fields and bad values are `bad_params`, nothing
changes) and returns the new state. Prefer the equivalent command (`view.edit`, `pane.tags`) when
there is one: commands are what users trigger and what parity tracks.

### 6.6 `ui.inspect` (with perf counters)

Everything in `ui.state`, plus:

| Field | Content |
|---|---|
| `window` | `{width, height, pixelsPerPoint, focused, drawing}` — `drawing` false while the compositor delivers no frames |
| `regions` | `[x, y, w, h]` per region of `ui-layout.md` §2: `menuBar`, `toolbar`, `tabStrip`, `tree`, `splitter`, `lightTable`, `scrollbar`, `sidePane`, `statusLine`, `tray`, `trayButtons`, `bottomControls`, `actionButtons`; in the edit view `editPanel`, `canvas`, `filmstrip`, `captionBar`; `null` when not shown |
| `scroll` | `{lightTable, tree, topSection, sticky}` — offsets in points, the section at the top, the pinned header |
| `hovered` / `focused` | widget ids |
| `popup` | the open menu, context menu or dropdown: `{kind, items: [{widget, id, label, enabled, checked?}]}` |
| `dialog` | the top dialog: `{kind, title, fields: {name: value}, buttons: [widget ids], default}` |
| `filePicker` | the stubbed picker waiting for `ui.filePicker` (§5.6) |
| `status` | the status-line text as drawn; `toasts` with their text and remaining time |
| `tooltip` | the tooltip text currently shown |
| `drag` | a drag in progress: `{from, ids, target, op, pill}` |
| `animations` | running animations (scroll-to-section, pane slide, fades) |
| `jobs` | same entries as `jobs.list` |
| `notices` | warnings waiting to be shown (damaged settings file kept aside, …) |
| `libraryProblem` | §5.9 |
| `perf` | below |

`perf` (rolling window of the last 240 frames; `resetPerf: true` clears the maxima):

| Counter | Meaning |
|---|---|
| `frameMs`, `frameP95Ms`, `maxFrameMs` | UI time per frame (layout + paint tessellation) — budget p95 ≤ 8 ms at 100 k photos (`development.md` §8) |
| `logicMs`, `updateMs`, `fps` | per-frame logic before layout; both together; frames per second |
| `cellsVisited`, `sectionsVisited`, `treeRowsVisited` | items laid out last frame — must stay flat as the library grows |
| `thumbRequests`, `jobsQueued` | jobs requested last frame; queue length per priority (`editMain`, `hover`, `visible`, `filmstrip`, `effectTiles`, `prefetch`, `background`) |
| `jobsInFlight`, `decodesInFlight`, `jobsDone`, `staleDropped` | job-pool activity |
| `textures`, `textureBytes`, `textureEvictions` | live egui textures and their size |
| `fsCachedPending` | file-system answers the UI is waiting for (`fs_cached`) |
| `opsApplied` | scanner `Op`s applied last frame |
| `memory` | `{budgetBytes, usedBytes, byCache: {…}}` (`architecture.md` §5) |
| `gpu`, `gpuFallback` | adapter in use; latest CPU fallback and its reason (`development.md`) |

### 6.7 `ui.widgets`

```json
[{"id": "tree:folder:1", "rect": [0, 128, 240, 24], "enabled": true, "label": "Lisbon Weekend (48)", "role": "treeRow"},
 {"id": "thumb:3", "rect": [282, 230, 144, 108], "enabled": true, "role": "thumbnail", "checked": true}]
```

Widgets drawn in the **last frame**, in paint order, with screen-point rects (clipped to what is
visible). `filter` is a substring of the id; `region` restricts to a region of §6.6. `checked` is the
selected/toggled state where one exists. Off-screen thumbnails are not listed: scroll first
(`ui.clickWidget` on the tree row, or `ui.scroll`). Ids are in §7.

### 6.8 Clicks, drags, hover and `ui.pointer`

- `ui.clickWidget {id}` clicks the widget's centre; `fx`, `fy` (0..1) pick another point inside it;
  `count: 2` double-clicks; `button: "right"` opens context menus; modifier flags apply to the click
  (`ctrl: true` toggles a thumbnail in the selection, `shift: true` extends it).
- `ui.dragWidget {id, toWidget}` drags from one widget onto another — e.g. `thumb:5` onto
  `tree:folder:2` moves the photo (hold `ctrl: true` to copy), onto `tray` adds it, onto
  `tree:album:a91c0e` adds it to the album. `toX`/`toY` (screen points) or `dx`/`dy` give a point
  instead. `steps` (default 12) intermediate moves; the drag pill and drop highlight run as for a
  user.
- `ui.hoverWidget {id, ms?}` rests the pointer on the widget, then waits `ms` (e.g. 650 for the
  600 ms thumbnail tooltip).
- `ui.pointer {events: [{kind: "move" | "down" | "drag" | "up", x, y}], space}` plays a raw gesture.
  `space: "screen"` (default) uses screen points; `space: "image"` uses **normalised coordinates of
  the photo shown in the edit view** (0..1, after rotation, as stored in recipes), for crop handles,
  Retouch strokes and crosshair effects (M7). `button: "right"` for
  secondary-button gestures. The rubber band on empty light-table space is a `down`/`drag`/`up` in
  screen space.

### 6.9 Keys and text

- `ui.key {key, ctrl?, shift?, alt?, cmd?}` presses and releases a key. Key names are egui's
  (`A`, `Enter`, `Escape`, `ArrowLeft`, `F2`, `Delete`, `Backslash`, …) plus the aliases `esc`,
  `return`, `left`, `space`.
- `ui.key {combo: "Ctrl+Shift+R"}` uses the shortcut syntax of `menus-and-shortcuts.md`, where
  **`Ctrl` means the command modifier** (Cmd on macOS), exactly as the docs write shortcuts. Use
  `macCtrl: true` for the physical Control key on macOS.
- `state: "down"` / `"up"` presses or releases without the other half, for the hold gestures:
  Ctrl+Alt full-screen preview, Shift over an effects tab (alternate effects, M12), `\` before/after
  (M11). A key left down is released when the connection closes.
- `ui.type {text}` types into the focused widget (search field, caption bar, rename dialog).

### 6.10 Menus

`ui.menu.tree` returns the menu bar as currently shown — including the fourth menu's live title
(**Folder** or **Album**), check marks, radio groups and disabled states — generated from the same
model as the in-window menus and the native macOS menu bar:

```json
[{"label": "File", "children": [
   {"id": "album.new", "label": "New Album…", "shortcut": "Ctrl+N", "enabled": true, "live": true, "widget": "menuitem:album.new"},
   {"separator": true},
   {"label": "Export Pictures to Folder…", "id": "output.export", "shortcut": "Ctrl+Shift+S", "enabled": false, "live": true}]}]
```

`ui.menu.tree {context: "photo" | "section" | "tray" | "treeGroup" | "tag" | "face"}` returns that
context menu's model for the current selection without opening it.

`ui.menu.invoke {path: "Picture/Batch Edit/Rotate Clockwise"}` (labels as shown, without a
trailing `…`, separated by `/`; a literal `/` in a label is `\/`) or `{id: "photo.rotate.cw"}`
behaves like clicking the item: items whose label ends in `…` open their dialog (reply
`{dialog: kind}`), others run their command (reply = the command's result, with `wait` as in
§5.3). Disabled items fail with `disabled`; catalog items that are not live yet fail with
`unsupported`.

Dialogs are driven through their widgets (`field:dialog.<name>`, `button:dialog.<action>`) or the
shortcuts `ui.dialog.confirm` (the default button, or `{button: "dialog.undoSave"}`) and
`ui.dialog.cancel`.

### 6.11 `ui.screenshot`

```text
→ {"id": 12, "method": "ui.screenshot", "params": {"path": "/tmp/after.png"}}
← {"id": 12, "ok": true, "result": {"path": "/tmp/after.png", "width": 1400, "height": 900, "headless": false}}
```

- Captures the whole window as a PNG at physical pixels, **after a settle** (§6.12, up to 5 s) so
  visible thumbnails and previews are finished; `settle: false` captures the next frame as is.
- `widget` or `region` (a region name or `[x, y, w, h]` in points) crops the capture.
- Without `path` the PNG comes back base64-encoded in `png` (subject to the 8 MiB reply limit).
  `path` follows §5.7 and must end in `.png`.
- `headless: true` draws the UI on the CPU (§8) instead of reading back the window: it needs no
  compositor and works when the window is occluded, minimised or the display is asleep. A windowed
  capture that receives no frame within **2 s falls back to headless automatically** (`headless:
  true` in the result).
- Dalica **never raises or focuses its window** to take a screenshot: stealing focus from the user's
  terminal is as bad as synthetic OS input.

### 6.12 `ui.settle` and `ui.wait`

`ui.settle {timeoutMs: 5000}` runs frames until nothing visible is pending for 12 consecutive
frames: no queued synthetic input, no high-priority (`editMain`, `hover`, `visible`, `filmstrip`)
jobs in flight, no running animation, no pending `fs_cached` answers, no scanner `Op` batch waiting.
Background thumbnail generation and long jobs are not waited for unless `jobs: true`. Reply:
`{settled, frames, ms, busy: ["visible thumbnails: 12", …]}` (`busy` says what was still running when
`settled` is false). Tests use `step_until(condition)` rather than counting quiet frames
(`development.md` §6); scripts use `ui.settle` before every screenshot.

`ui.wait {ms}` lets time pass: in the window it runs frames for `ms` of real time; headless it
advances the virtual clock in 16 ms frames (tooltips, auto-repeat of the section buttons,
slideshow timing).

### 6.13 Jobs

```json
{"job": 7, "kind": "export", "label": "Exporting 12 of 48", "state": "running",
 "progress": {"done": 12, "total": 48}, "cancellable": true, "startedMs": 1840}
```

`state` is `queued | running | done | failed | cancelled`; finished jobs keep `result` or `error`
and stay listed for 10 minutes (at most 64 entries). Kinds: `scan`, `import`, `export`, `save`,
`batchEdit`, `metadataWrite`, `rebuild`, `duplicates`, `faces`, `collage`. `jobs.cancel` stops a job at its next photo boundary: photos already written stay
(each was written atomically), nothing is left half-written. Cancelling a job is never an undo of
the photos it completed.

## 7. Widget ids

Every interactive widget registers an automation id with its rect each frame (`architecture.md`
§7). Ids are stable across releases and never translated (`localization.md`); renaming one is a
breaking change for scripts and tests.

Grammar: `kind ":" name ("/" part)*` — `kind` is lowercase; `name` is a command-like dotted name or
an id; `part` names a sub-widget of a composite (section headers). Effect and slider names are the
recipe operation and parameter names of `data-model.md` §3.1 and `editing.md` (snake_case).

| Region (milestone) | Ids |
|---|---|
| Regions (M3) | `region:<name>` for each region of §6.6 shown (`region:menuBar`, `region:toolbar`, `region:tree`, `region:splitter`, `region:lightTable`, `region:scrollbar`, `region:sidePane`, `region:statusLine`, `region:tray`, `region:trayButtons`, `region:bottomControls`, `region:actionButtons`): the region's rect, no input of its own (role `other`); a region not shown is not listed |
| Menu bar (M3) | `menu:File` … `menu:Help` (title by its English label; the fourth is `menu:Folder` whatever its shown title), `menuitem:<command id>` for open menu and context-menu items, `submenu:<path>` |
| Toolbar (M3) | `button:toolbar.import`, `button:toolbar.newAlbum`, `button:tree.flat`, `button:tree.tree`, `button:toolbar.viewOptions`, `button:filter.starred`, `button:filter.faces`, `button:filter.videos`, `button:filter.geotagged`, `slider:age`, `field:search`, `button:search.clear`, `indicator:activity` |
| Room tabs (M11+) | `tab:room.library`, `tab:room.import`, `tab:room.collage`, …; `button:room.close.<room>` |
| Library tree (M4) | `tree:group:albums` · `people` · `projects` · `folders` · `other` · `hidden` (the group header), `tree:folder:<FolderId>`, `tree:album:<AlbumId>` (also `tree:album:starred`, `tree:album:recent`, `tree:album:search`), `tree:person:<PersonId>` (M13), `tree:disclosure:<FolderId>` (tree mode), `splitter:tree` |
| Light table (M4) | `section:<section id>` (the header block, e.g. `section:folder:12`), its parts `section:folder:12/icon`, `/title`, `/description`, `/play`, `/collage`, `/selectStarred`, `/save`, `/export`; `thumb:<PhotoId>` and its star badge `thumb:<PhotoId>/star`; `lightTable` (empty space: rubber band, section menu); `banner:search` and `button:search.showAll` (M9) |
| Scrollbar (M4) | `scroll:prevSection`, `scroll:up`, `scroll:track`, `scroll:thumb`, `scroll:down`, `scroll:nextSection` |
| Status line (M4) | `text:status`, `indicator:unsaved`, `indicator:temporarySession` |
| Photo Tray (M4) | `tray` (the box), `tray:token`, `tray:item:<PhotoId>`, `button:tray.hold`, `button:tray.clear`, `button:tray.addToAlbum` |
| Bottom controls (M3) | `button:star`, `button:rotate.ccw`, `button:rotate.cw`, `button:loupe`, `slider:thumbSize`, `toggle:pane.people`, `toggle:pane.places`, `toggle:pane.tags`, `toggle:pane.properties`; edit view: `button:zoom.fit`, `button:zoom.1to1`, `slider:zoom` |
| Action buttons (M3) | `action:export`, `action:collage`, `action:slideshow`, `action:more` |
| Side pane (M6) | `pane:tags`, `pane:people`, `pane:places`, `pane:properties` (the pane body), `button:pane.widen`, `button:pane.close`; Tags: `field:tags.add`, `button:tags.add`, `tag:<tag text>`, `quicktag:1` … `quicktag:10`, `button:quicktags.configure`; Properties: `button:properties.showAll` |
| Edit view (M7) | `button:edit.back`, `button:compare.one`, `button:compare.two`, `canvas:edit` (the photo), `film:<PhotoId>`, `field:caption`, `tab:edit.basic`, `tab:edit.tuning`, `tab:edit.effects`, `tab:edit.effects2`, `tab:edit.effects3`, `button:edit.undo`, `button:edit.redo`, `button:edit.showOriginal`, `box:histogram` |
| Edit tools (M7) | `tool:crop`, `tool:straighten`, `tool:autofix`, `tool:autocontrast`, `tool:autocolor`, `tool:retouch`; `slider:basic.fill_light`; tool panels: `slider:straighten.angle`, `choice:crop.ratio`, `button:crop.rotate`, `button:panel.apply`, `button:panel.cancel`, `button:panel.reset` |
| Tuning (M7) | `slider:tuning.fill_light`, `slider:tuning.highlights`, `slider:tuning.shadows`, `slider:tuning.temperature`, `button:tuning.neutralPicker`, `button:tuning.reset` |
| Effects (M12) | `effect:<name>` per tile (`effect:sharpen`, `effect:sepia`, `effect:bw`, `effect:soft_focus`, `effect:graduated_tint`, …); effect panels: `slider:effect.<param>` (`slider:effect.amount`, `slider:effect.fade`), `color:effect.<param>`, `check:effect.<param>`, `crosshair:effect` |
| Dialogs (M3+) | `dialog:<kind>` (the dialog), `field:dialog.<name>`, `check:dialog.<name>`, `choice:dialog.<name>`, `button:dialog.ok`, `button:dialog.cancel`, `button:dialog.<action>` (`button:dialog.undoSave`, `button:dialog.clearTray`) |
| Toasts (M3) | `toast:<n>`, `button:toast.<action>` (`button:toast.showFolder`) |

Rules for implementers:

- Register through the one helper in `widgets.rs` (never by hand); a widget that is drawn but not
  registered is a bug caught by the headless "every interactive widget has an id" test.
- An id is unique within a frame. Repeated widgets carry their model id (`thumb:<PhotoId>`), never a
  position index.
- New regions add their ids to this table in the same commit.

## 8. Headless rendering (no window, no GPU)

The egui output is rasterized on the CPU (`crates/ui-kit/src/raster.rs`, driven by
`crates/ui/src/headless.rs`; our own, modelled on LightCraft's): same tessellation, gamma-space premultiplied
blending and scissor clipping as egui's GPU backends, so the image matches the window (minus GPU
dithering). Thumbnails and previews come from the same job pool, drained while settling.

- **In the running app:** `ui.screenshot {headless: true}` draws the current UI into an offscreen
  context from the logic tick; windowed screenshots fall back to it after 2 s (§6.11).
- **Without any window:** `dalica-cli snapshot` runs a whole app session headlessly and answers the
  same requests through the same handler (no TCP, no token):

```sh
dalica-cli snapshot --demo -o grid.png --size 1400x900
dalica-cli snapshot --demo --script tour.jsonl -o out.png --size 1400x900 [--scale 2] [--now 2026-06-10T12:00:00]
dalica-cli snapshot --library DIR --script tour.jsonl -o shot.png      # an existing library, read-only
dalica-cli snapshot ~/Pictures/samples --script tour.jsonl -o shot.png # these folders, Scan Once, in memory
cargo xtask snapshot tour.jsonl                                         # same, output under target/snapshots/
```

- **Script format:** one request per line, `{"method", "params"}` or the shorthand
  `{"command", "params"}` for `engine.execute`; `#` comment lines and blank lines are skipped.
  Replies are printed to standard output as JSON lines `{"line", "method", "ok", "result" | "code" +
  "error", "ms"}`.
- A failed request does not stop the script — the remaining lines and the final screenshot still
  run — but the exit status is **1** when any request failed (2 for usage errors), so CI can judge
  a snapshot by its exit status.
- `ui.screenshot` without `path` writes `-o`, then `OUT-2.png`, `OUT-3.png`, …; a script without any
  screenshot gets one at the end to `-o`. Each request runs frames until it is answered and its
  input has played out (§5.2).
- **Deterministic:** a virtual clock (16 ms per frame), a fixed "now" (`--now`, default
  `2026-06-10T12:00:00` local), the seeded demo library and settled renders make the same script
  produce the same pixels on every machine with the same fonts, which is what fingerprint tests
  and the showcase rely on.
- **Isolated:** a snapshot session never reads or writes the user's settings, window state or
  library. `--library DIR` opens that library **read-only** (no sidecar, metadata or index writes;
  a locked library is opened as a read-only copy of its snapshot). `--demo` uses the persistent demo
  library (§1), so a script's stars stay in it; `--demo-fresh` writes the procedural photos into a
  new temporary folder removed on exit, the clean slate a deterministic script wants.
- **Paths:** the person running `snapshot` chose its output, so the working directory and the
  directory of `-o` are write roots of a snapshot session, and relative `path` params resolve against
  the working directory (the other rules of §5.7 still apply). `dalica-cli run --connect` likewise
  makes relative `path` params absolute against its own working directory before sending them; the
  app then checks them against its own roots.
- Size: `--size WxH` in points (default 1400x900), `--scale` pixels per point (default 1). Limits as
  in §4; they are checked before layout. `ui.resize` works inside scripts too.
- A 1400 × 900 demo snapshot should take ≤ 2 s in a release build; `cargo xtask bench` tracks it.

**Rust test driver:** headless UI tests use the same driver directly:
`Headless::new(app, size, scale)`, `request(method, params, timeout)` (runs frames until answered
and the input has played out), `settle(timeout)`, `step_until(timeout, |h| condition)` and `paint()`.
Assert on `library.inspect` / `ui.inspect` and perf counters, not on pixel counts, except in
fingerprint tests (`architecture.md` §11).

## 9. Examples

### 9.1 A minimal client (Python, for scripts outside the repo)

```python
#!/usr/bin/env python3
"""dalica_ctl.py PORT FILE.jsonl: send control requests, print the replies."""
import json, os, socket, sys
port = int(sys.argv[1])
xdg = os.environ.get("XDG_RUNTIME_DIR")   # Linux; see §2.1 for macOS and Windows
run = f"{xdg}/dalica" if xdg else os.path.expanduser("~/.cache/dalica/run")
token = open(os.environ.get("DALICA_CONTROL_TOKEN_FILE",
             f"{run}/control-{port}.token")).read().strip()
s = socket.create_connection(("127.0.0.1", port), timeout=120)
f = s.makefile("rw", encoding="utf-8", newline="\n")
def call(method, params=None, _id=[0]):
    _id[0] += 1
    f.write(json.dumps({"id": _id[0], "method": method, "params": params or {}}) + "\n"); f.flush()
    return json.loads(f.readline())
assert call("auth", {"token": token})["ok"]
for line in open(sys.argv[2]):
    line = line.strip()
    if not line or line.startswith("#"): continue
    r = json.loads(line)
    method, params = ("engine.execute", {"command": r["command"], "params": r.get("params", {})}) \
        if "command" in r else (r["method"], r.get("params"))
    print(method, json.dumps(call(method, params))[:200])
```

Inside the repository prefer `dalica-cli run --connect 127.0.0.1:7990 --script FILE.jsonl` (same
script format as `snapshot`, token read from the default token file; `--keep-going` to continue
after a failure).

### 9.2 Tour: select across folders, hold, edit, look

```jsonl
# tour.jsonl — demo library; works headless and against `dalica --control … --demo`
{"method": "ui.resize", "params": {"width": 1400, "height": 900}}
{"method": "ui.clickWidget", "params": {"id": "tree:folder:1"}}
{"method": "ui.clickWidget", "params": {"id": "thumb:2"}}
{"method": "ui.clickWidget", "params": {"id": "thumb:4", "ctrl": true}}
{"method": "ui.clickWidget", "params": {"id": "button:tray.hold"}}
{"method": "ui.clickWidget", "params": {"id": "tree:folder:2"}}
{"method": "ui.clickWidget", "params": {"id": "thumb:51"}}
{"method": "library.inspect"}
{"method": "ui.settle"}
{"method": "ui.screenshot", "params": {"path": "/tmp/tray-held.png"}}
# open the edit view on the last photo and try Auto Fix, then look
{"method": "ui.key", "params": {"combo": "Ctrl+3"}}
{"method": "ui.clickWidget", "params": {"id": "tool:autofix"}}
{"method": "ui.inspect"}
{"method": "ui.screenshot", "params": {"path": "/tmp/edit-autofix.png"}}
{"method": "ui.key", "params": {"key": "Escape"}}
```

### 9.3 The agent loop for a UI change

1. Start `cargo run --release -p dalica -- --control 18042 --demo` (own port), or use
   `dalica-cli snapshot` when no live window is needed.
2. `engine.commands {filter: "star"}` to find ids; `library.inspect` and `ui.widgets` to find
   targets.
3. Act with `ui.clickWidget` / `ui.key` / `ui.menu.invoke` (to test the UI path) or `engine.execute`
   (to set up state quickly).
4. `ui.settle`, then `ui.screenshot` at 1400 × 900 and again after `ui.resize {900, 600}`; open the
   PNGs and compare with `ui-layout.md`.
5. `ui.inspect` → `perf`: `frameP95Ms` within budget, `cellsVisited` flat.

## 10. Rules for agents

- **Never send OS-level synthetic input** — no xdotool, ydotool, wtype, osascript, AutoHotkey,
  PowerShell `SendKeys`, accessibility-API clicks or window-manager key injection. It goes to
  whatever window is focused, including the user's terminal or editor, and can type into it. Every
  click, key and gesture an agent needs exists in this protocol; if one is missing, add a method or
  a widget id instead.
- Never focus, raise, move or resize the user's other windows; Dalica's own window size is set with
  `ui.resize`.
- Use your own port (18000–19999) and token file when running in parallel; never connect to the
  maintainer's instance on 7990 unless asked.
- Prefer `--demo` or a scratch library. Never point automation at a real personal photo library
  without the maintainer asking; when you must, start with `dalica-cli snapshot --library DIR`
  (read-only).
- Inspect before retrying after a `timeout` or a dropped connection: the command may have run.

## 11. Security notes

- The token proves possession of a secret; it is not per-method authorization. A client with the
  token gets the whole control surface (except what §5.5–§5.7 refuse). Keep token files private.
- The protocol is unencrypted and listens on loopback only. Never tunnel, proxy or port-forward it.
- Requests are hostile input (`AGENTS.md` §3): every parameter is validated before anything
  changes, numbers are range-checked, lists are capped, and `panic_hunt` fuzzes every method as it
  fuzzes every command.
- Replies and logs never contain the token, face descriptors or (without
  `--automation-allow-people`) people names. Screenshots show whatever the window shows.
- Security-relevant changes to this channel follow `contributing.md` (state whether the change adds
  a control or documents a limitation).
