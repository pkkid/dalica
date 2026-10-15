# Control protocol

Adapted from LightCraft `docs/control-protocol.md` (revision 88ea1a8) and PhotoCraft
`docs/control-protocol.md` (revision 59684266); MIT OR Apache-2.0.

> **Status: built** for what is not marked planned: the headless renderer and
> `dalica-cli snapshot` (M1), the control channel, widget registry and `ui.screenshot` (M2), the
> menu methods (M3); see [`roadmap.md`](roadmap.md). Methods and widget ids that
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
  `timeout`. Follow up with `jobs.wait` / `jobs.list`. **Planned, not built yet:** today the
  scans (`folders.add`, `library.rebuild`) reply at once (`scanning: true`) and do not name their
  job; `jobs.list` lists it, and `jobs.wait` waits for it.
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
user. `ui.menu.invoke` and clicks open the real confirmation dialog instead. Some commands ask only
sometimes: `tray.clear` asks only when it would empty a tray holding held photos. Those ask through
the in-app dialog host: run without `confirm` from the UI they open their question
(`{dialog: "clearTray"}`), and `prompt` names the dialog (`{title, text, buttons, dialog}`). Deletes still go to the
OS trash; `confirm` never turns a trash delete into a permanent one.

M8's questions ask through the host too: `photo.save` opens Save's first-time explanation
(`{dialog: "saveExplain"}`, buttons Save and Cancel; its "Don't show this again" is the view
state's `saveExplain.dontShow`, and Save then runs `photo.save` with `"confirm": true` and
`"dontAsk": true`, which stops the engine asking), and `photo.revert` opens the Revert question
(`{dialog: "revert"}`, `prompt.buttons` `["Revert", "Undo Save", "Cancel"]`; Revert runs
`photo.revert` with `"confirm": true`, Undo Save with `"action": "undoSave"` as well). The Revert
question has no default button: Enter and a bare `ui.dialog.confirm` revert nothing; name the
button (`{button: "dialog.revert"}`). In the edit view these commands, and `photo.undoSave` and
`photo.saveCopy`, act on the photo shown when they are run without `ids` (File ▸ Save, Ctrl+S).
There Undo after a Save, with no edit made since, is Undo Save: exactly when `photo.history.state`
answers `undoSave`, Ctrl+Z (`edit.history.undo`), Edit ▸ Undo and `edit.undo` run `photo.undoSave`
on the photo shown; otherwise they undo its last edit.

M9's questions ask through the host the same way, each once its engine command is registered
(until then running it is `unknown_command`, never a question whose answer could not run):
`album.delete` (`{dialog: "deleteAlbum"}`, [Delete Album] [Cancel]), `album.remove`
(`{dialog: "removeFromAlbum"}`, "Remove the %d selected pictures from this album? The files are not
deleted." [Remove] [Cancel]), `photo.delete` (`{dialog: "deletePhotos"}`, "Move the %d selected
pictures to the trash?" [Move to Trash] [Cancel]; its draft is `deletePhotos`), `folder.delete`
(`{dialog: "deleteFolder"}`, naming the folder; its draft is `deleteFolder`) and `photo.moveTo`
(`{dialog: "moveFiles"}`, "Move these files on disk? The files will be moved, not copied." [Move]
[Cancel] with "Don't ask again", `check:dialog.dontAsk`, the view state's `moveFiles.dontAsk`; Move
runs `photo.moveTo` with `"confirm": true`, and `"dontAsk": true` when the box is ticked). Copying
never asks in the engine: the UI asks "Copy %d files to %s?" (`dialog:copyFiles`) before it runs
`photo.copyTo`, and a name clash asks [Rename Duplicates] [Skip Duplicates] [Cancel]
(`dialog:collision`, which runs the move or copy again with `"collisions": "rename" | "skip"`).

### 5.6 Effects outside Dalica are refused by default

Automation must not reach beyond the app. Without `--automation-allow-external`, these refuse with
`external_denied`: Locate on Disk (opens the file manager), Open in External Editor, Check for
Updates, opening the help or release-notes web pages, and anything that would
use the network (map tiles, geocoding, model downloads; `AGENTS.md` "Privacy").

**Native file pickers are never opened by automation.** `engine.execute` takes paths as params. A
picker requested by synthetic input (a *Browse…* button clicked through `ui.clickWidget`) is
replaced by an in-app stub: `ui.inspect` → `filePicker: {command, kind: "folder" | "file" | "save",
title}`, answered with `ui.filePicker {path}` or `ui.filePicker {cancel: true}`. Pickers opened by
real user input stay native.

The save picker (File ▸ Save As…, `photo.saveAs`, `ui.menu.invoke` replies `{dialog:
"filePicker"}`) adds what it suggests and what its answer runs: `filePicker: {command:
"photo.saveTo", kind: "save", title: "Save As", name: "IMG_0041.jpg", folder, params: {id}}`.
`ui.filePicker {path}` runs `command` with `params` plus `path`, once the path passes the writing
rule of §5.7 (a refused path is the reply and the picker stays open); `{cancel: true}` closes it
and changes nothing. One native picker is open at a time. The app follows the save its picker's
answer started: when the job ends with the code `exists` (or `no_folder`), or `photo.saveTo`
refuses the name (`bad_params`), the picker opens again, so `filePicker` reappears: titled
"Choose Another Name" with the name tried, in its folder, for a taken name; "Save As" in the
photo's folder for a missing one; the toast of the failed save names the file tried, not the
photo. A `photo.saveTo` an agent runs itself opens no picker.

The Export dialog's *Browse…* (`button:dialog.browse`, M8.20) opens a folder picker for the export
location: `filePicker: {command: "output.export.location", kind: "folder", title: "Export
Location", folder, params: {}}`. Its answer is checked as a folder new files are made in (§5.7
writing, as `export.run`'s `dir`) and runs `output.export.location {path}`, which sets the open
dialog's location; it never adds a watched folder (only File ▸ Add Folder…'s picker runs
`folders.add`).

### 5.7 Paths

All path params (folder to add, export target, screenshot `path`, render `path`) are absolute,
normalised paths; `..` components are refused; symlinks are resolved before the check.

- **Reading** (adding a watched folder, `library.openFile`, import from a folder): the path must be
  inside an `--automation-read-root` or an already watched folder. With no read root, adding new
  folders over automation is refused (`path_denied`), except in `--demo`.
- **Writing new files** (export, collage, screenshots, renders): the target must be inside an `--automation-write-root`. With no write root
  given, the defaults are the OS temporary directory and `<cache>/Dalica/automation/`.
- Never inside a watched folder or any `.dalica-originals/` (a screenshot or render there would turn
  into a library photo or touch a backup), and never over an existing file, except that a screenshot
  or render may replace an existing `.png`. Exports never overwrite (collisions get `-001`, `data-model.md` §6).
- **New files meant for the library**: `export.run`'s `dir` (the folder the export folder is made
  in; it need not exist yet) and `photo.saveTo`'s `path` (Save As, and the save picker's answer)
  may also lie inside a watched folder, since an export and a saved copy join the library by
  design; they are never inside `.dalica-originals/`, a Save As path never names an existing file
  and its folder must exist. These two are checked before anything else about the command (an
  unknown or disabled command with a denied path answers `path_denied`).
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
| `ui.dialog.confirm` / `ui.dialog.cancel` | `{button?}` / — | what the dialog's button ran / `{}` (§6.10) |
| `ui.filePicker` | `{path} \| {cancel: true}` | the command's result (§5.6) |
| `ui.screenshot` | `{path?, headless?, widget?, region?, scale?, settle?}` | `{path, width, height, headless}` (§6.11) |
| `ui.settle` | `{timeoutMs?, jobs?}` | `{settled, frames, ms, busy}` (§6.12) |
| `jobs.list` | — | `[{job, kind, label, state, progress, result?, error?}]` (§6.13) |
| `jobs.wait` | `{job, timeoutMs?}` | the job's final entry, or its running entry with `pending: true` |
| `jobs.cancel` | `{job?}` | `{cancelled: [job…]}`; no `job` = every cancellable job |
| `app.quit` | — | `{}`; pending sidecar/metadata writes are flushed first |

There are no hidden aliases: a misspelt method is `unknown_method`.

### 6.1 `engine.execute`

```json
{"id": 3, "method": "engine.execute", "params": {"command": "photo.caption.set", "params": {"ids": [7], "caption": "Tram 28 at sunset"}}}
{"id": 4, "method": "engine.execute", "params": {"command": "export.run", "params": {"album": "a91c0e", "dir": "/tmp/out", "folderName": "Lisbon Weekend", "size": 1600}, "wait": false}}
```

Runs an engine command or a `UI_COMMANDS` entry (`view.edit`, `pane.tags`, `view.thumbsSmall`)
by id. A UI command that opens a dialog opens it here too (`output.export` replies `{dialog:
"export"}`); the export itself is the engine's `export.run`, as above. `params` is the command's own JSON, documented by its `params` string in `engine.commands`.
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
  "search": {"active": true, "mode": "search", "text": "lisbon", "filters": {"starred": false, "faces": false,
             "videos": false, "geotagged": false}, "age": 0, "matches": 33, "folders": 4, "bytes": 439800000,
             "seconds": 0.01, "sections": [{"section": "album:search", "count": 33}, {"section": "folder:1", "count": 30}],
             "unavailable": [], "truncated": false, "pending": false, "generation": 7, "groups": null, "banner": null},
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
- `search` is search mode (`organizing.md` §8, `ui-layout.md` §9): `active` and `mode` (`"search"`,
  `"duplicates"` for Find Duplicates' groups, `null` while not searching); the query, `age` in days
  (0: every age; the toolbar's slider is a position in `ui.state` → `search.age`, turned into days
  on its logarithmic scale); and once the worker's answer came the banner's numbers: `matches`
  (`null` before it and outside search mode), `folders`, `bytes`, `seconds`, the `sections` that
  keep a match with their counts (the search album first), `unavailable` (operators and filters
  that match nothing yet: `person`, `color`, `faces`) and `truncated` (the text was cut at the
  grammar's caps). `pending` is true while a query waits for its debounce (120 ms on the session
  clock) or for the worker; `generation` rises with every change of the query. In duplicates mode
  `groups` lists the groups (`{count, size, bytes, ids}`, largest total first) and `banner` is the
  banner's text ("12 duplicate groups (31 files)"); both are `null` otherwise. `ui.settle` does not
  wait for a pending search: poll `pending`.
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
 "folder": null, "folderSort": "date", "folderSortReverse": false,
 "collapsedGroups": [], "collapsedFolders": [], "smallPictures": false, "hiddenPictures": false,
 "selection": {"ids": [], "anchor": null, "section": null, "whole": false},
 "tray": {"held": [], "selected": []}, "scroll": {"section": null, "offset": 0},
 "edit": {"photo": null, "tab": "basic", "tool": null, "compare": "one", "zoom": "fit", "panelVisible": true,
          "crop": null, "straighten": null, "retouch": null, "tuning": null},
 "slideshow": null, "language": "en", "status": "No selection", "dialogs": [], "activity": false,
 "captionEdit": null, "descriptionEdit": null, "propertiesShowAll": false,
 "tagsPane": {"draft": "", "highlight": null},
 "about": {"tab": "about", "list": "grabBag", "sort": "firstCommit", "ascending": true, "names": "username"},
 "export": null, "saveExplain": null, "revert": null, "exportProgress": null,
 "search": {"text": "", "highlight": null, "filters": {"starred": false, "faces": false, "videos": false, "geotagged": false},
            "age": 0, "savedScroll": null},
 "clipboard": null, "albumProperties": null, "renameFiles": null, "newFolder": null, "moveFiles": null,
 "copyFiles": null, "collision": null, "deletePhotos": null, "deleteFolder": null}
```

`view` is `library | edit | slideshow | viewer`; `room` is the active tab (`library`,
`import`, `collage`); `status` is the status line's text as drawn (read-only); `treeWidth` is
200 … W − 480 for the current window. The library view's fields (M4):

| Field | Values | Changed by |
|---|---|---|
| `treeMode` | `flat` \| `tree` | `view.folderView {mode}`, the toolbar's Flat \| Tree |
| `folderSort`, `folderSortReverse` | `date` \| `recent` \| `size` \| `name`; bool | `view.folderSort {by?, reverse?: true}` (the view-options popup; `reverse: true` flips) |
| `collapsedGroups` | group names as in `tree:group:<name>`: `albums`, `people`, `projects`, `folders`, `other`, `hidden` | `view.groupCollapse {group, collapsed?}` |
| `collapsedFolders` | `[FolderId]`, tree mode's folded folders | `view.folderCollapse {folder, collapsed?}` |
| `caption` | `none` \| `filename` \| `caption` \| `tags` \| `resolution` \| `date` | `view.caption {caption}` |
| `smallPictures`, `hiddenPictures` | bool | `view.smallPictures {on?}`, `view.hiddenPictures {on?}` (toggle without `on`) |
| `selection` | `ids`, `anchor`, `section` (the section the photos are in, written as its widget id, `"folder:12"`; `null` for none), `whole` (the section selected as a unit) | the `select.*` commands only (read-only here) |
| `tray` | `held` (photos kept by Hold), `selected` (the tray's own selection: photos the tray shows) | the `tray.*` commands |
| `scroll` | `section` (the section at the top of the light table, `null` for the top of the library) and `offset` (points below that section's top, 0 … 10⁹) | the scroll commands, the scroll bar |
| `dialogs` | the open in-app dialogs, bottom first (the last takes input): `{kind, fields, command?}`, `kind` as in `dialog:<kind>` (`clearTray`, `folderManager`, `firstRun`, `removeFolder`, `watchDrive`; M6: `folderProperties`, `adjustDate`, `quickTags`, `renameTag`, `about`, `releaseNotes`; M7: `customRatio`; M8: `saveExplain`, `revert`, `export`; M9: `albumProperties`, `removeFromAlbum`, `deleteAlbum`, `moveFiles`, `copyFiles`, `collision`, `newFolder`, `renameFiles`, `deletePhotos`, `deleteFolder`), `fields` its draft (nothing is applied before its default button), `command` what a confirmation runs when confirmed | read-only here: the commands that open them (`folders.manager`, a `tray.clear` that asks), their buttons, `ui.dialog.confirm`, `ui.dialog.cancel` |
| `activity` | bool: the activity popover under `indicator:activity` is open (its rows are `jobs.list`'s) | `ui.set`; a click on the indicator (M5.19) |

M6's fields (all start fresh; none is persisted):

| Field | Values | Changed by |
|---|---|---|
| `captionEdit` | `null`, or `{photo, draft}`: the inline caption editor under that photo's cell and the text typed so far (at most 4096 characters; nothing is applied before it commits through `photo.caption.set`) | `photo.caption.edit {id?}` (Picture ▸ Edit Caption: the focused photo unless `id` names one), typing in `field:caption.inline`, Enter / Esc |
| `descriptionEdit` | `null`, or `{section, draft}`: the inline description editor of that section's header (`"folder:12"`) and its text (at most 4096 characters; commits through `folder.info.set`) | a click on `section:<id>/description`, Enter / Esc |
| `propertiesShowAll` | bool: the Properties pane lists every metadata field | `button:properties.showAll` |
| `tagsPane` | `{draft, highlight}`: the Tags pane's "Add a tag:" text (at most 4096 characters) and the highlighted row of its autocomplete list (0 … 99, `null` for none) | typing in `field:tags.add`, ↑ / ↓ |
| `about` | `{tab, list, sort, ascending, names}`: the About dialog's tab (`about` \| `contributors` \| `models`), how Contributors lists people (`grabBag` \| `table`), its sort (`name`, `prs`, `commits`, `linesAdded`, `linesDeleted`, `linesDelta`, `binaryAdded`, `binaryDeleted`, `firstCommit`, `lastCommit`), the order and the name shown (`username` \| `displayName` \| `realName`); kept while the app runs, so the dialog reopens as it was left | `ui.set`; the dialog's tabs and controls |

M7's field, `edit` (starts fresh; not persisted): the edit view's state (`editing.md`).

| Field | Values | Changed by |
|---|---|---|
| `edit.photo` | the photo shown, `null` outside the edit view; `view: "edit"` needs one (`bad_params` without, `not_found` for a photo that does not exist) | `view.edit {id?}` (Ctrl+3 and Enter in the library, View ▸ Edit View, Picture ▸ View and Edit: the photo named, else the focused or first selected; with nothing selected the toast "Select a photo to edit."); `view.library` (Esc, Ctrl+1, Back to Library) sets `null` |
| `edit.tab` | `basic` \| `tuning` \| `effects` \| `effects2` \| `effects3` (as in `tab:edit.<tab>`) | `edit.tab {tab}`, the tabs |
| `edit.tool` | `null`, or the open tool: `crop` \| `straighten` \| `retouch` \| `tuning` (a Tuning session) | `edit.tool.open {tool}`, `edit.tool.apply`, `edit.tool.cancel` (Esc while a tool is open), leaving the edit view |
| `edit.compare` | `one` \| `two` (A│B) \| `same` (A│A) | the compare toggle (M11) |
| `edit.zoom` | `"fit"`, or the scale of the photo's actual pixels, 0.001 … 4 (1 is 100 %; a scale at or below the fitted one shows fitted) | `edit.zoom.fit` and `edit.zoom.actual` (the bottom area's Fit │ 1:1, animated), `edit.zoom.in` / `edit.zoom.out` (Ctrl+= / Ctrl+−, ×1.25 about the pointer), `edit.zoom.set {slider}` (the zoom slider: 0 fit, 0.5 100 %, 1 400 %), the wheel over the photo (×1.1 a notch about the pointer), a press held on the fitted photo (100 % there), a click on the zoomed photo (fit); steps land on 100 % when they cross it (`editing.md` §3). The pan is the canvas's own: `canvas:edit` and the navigator show it |
| `edit.panelVisible` | bool | `edit.panelToggle {on?}` (Ctrl+9, View ▸ Show Edit Controls) |
| `edit.crop` | `null`, or the Crop draft `{ratio, rect}`: `ratio` `manual` \| `current` \| `"W:H"` (`"4:6"`, `"8.5:11"`), `rect` `[x, y, w, h]` normalised to the photo in the stored frame (`data-model.md` §3.3) or `null` before one is drawn | the Crop tool |
| `edit.straighten` | `null`, or `{angle}`, −10 … 10 degrees | the Straighten tool |
| `edit.retouch` | `null`, or `{patches: [{target: [x, y], source: [x, y], radius}], position, brush}`: the patches made (at most 1000, points normalised to the photo, radius 0.002 … 0.25 of its longer side), how many are in effect (0 … the number of patches; Undo Patch lowers it) and the brush radius (0.002 … 0.25) | the Retouch tool |
| `edit.tuning` | `null`, or a Tuning session's `{fill, highlights, shadows, temperature, neutral}`: 0 … 1, 0 … 0.5, 0 … 0.5, −1 … 1, and the neutral colour `[r, g, b]` (each 0 … 1) or `null` | the Tuning tab |

M8's fields (all start fresh; none is persisted): the drafts of M8's dialogs, `null` while they are
closed (opening sets them, closing clears them), and the export progress window. `ui.set` merges
`export` field by field (a partial draft set over `null` takes the dialog's defaults); the others
are replaced whole.

| Field | Values | Changed by |
|---|---|---|
| `export` | `null`, or the Export dialog's draft (`creating.md` §2): `{ids, section, location, folderName, numbers, originalSize, resize, quality, custom, videos, watermark, watermarkText}`: the photos exported (`[PhotoId]`; a header's Export passes the selection inside its section) and the section (`"folder:12"` or `null`; with no `ids`, the whole section), Export location (a path, at most 4096 bytes), Name of exported folder, the numbers checkbox, Use original size (`true`) or Resize to, `resize` the size field's text as typed (at most 16 characters; Export takes digits 100 … 4000), `quality` `automatic` \| `normal` \| `maximum` \| `minimum` \| `custom`, `custom` 1 … 100, `videos` `firstFrame` \| `fullVideo`, the watermark checkbox and its text (texts at most 4096 characters). Export is enabled when there is something to export, a location, a folder name, a size it takes (or the original size) and, with the watermark on, its text; it runs `export.run` | `output.export {ids?, section?}` (File ▸ Export Pictures to Folder…, Ctrl+Shift+S, the bottom bar's and a section header's Export: the photos named, else the section, else the tray's photos; disabled with nothing to export), the dialog's controls; Export, Cancel and Esc close it |
| `saveExplain` | `null`, or `{dontShow}` while Save's first-time explanation is open | `photo.save` without `confirm` (§5.5), its checkbox |
| `revert` | `null`, or `{photos: [PhotoId]}` while the Revert question is open: the photos it reverts (those named, else the tray's) | `photo.revert` without `confirm` (§5.5) |
| `exportProgress` | `null`, or `{job, shown}`: the export job the progress window follows ("Exporting 12 of 48…"; a job `jobs.list` lists) and whether the window is shown. The window is not modal: the app under it keeps taking input, and the job runs on while it is hidden | the export job starting and ending (M8.21), its Hide |

M9's fields (all start fresh; none is persisted): the search box and its filters, the clipboard,
and the drafts of M9's dialogs, `null` while they are closed (opening sets them, closing clears
them). Every one is replaced whole by `ui.set`. Texts are at most 4096 characters (dates 64), lists
at most 1 000 000 photos; photos and folders must exist (`not_found`), and a section named in
`savedScroll` must be shown (a folder of the library, an album section the light table shows).

| Field | Values | Changed by |
|---|---|---|
| `search` | `{text, highlight, filters: {starred, faces, videos, geotagged}, age, savedScroll}`: what is typed in the search field, the highlighted autocomplete row (0 … 99, `null` for none), the filter toggles, the age slider's position (0 = everything … 1 = only the newest; the slider's logarithmic map turns it into an age) and the scroll anchor saved when search mode began (`null` outside it). Search mode is some text, a filter or an age: the light table then shows the 32-high filter banner (`region:banner`) above it | the toolbar's search field, filters and age slider (M9.22), `search.focus` (Ctrl+F, F3: the focus to the field) |
| `clipboard` | `null`, or `{op: "cut" \| "copy", ids}`: the photos Paste moves or copies | `photo.cut {ids?}` (Ctrl+X), `photo.copy {ids?}` (Ctrl+C): the photos named, else the selection; `photo.paste {folder?}` (Ctrl+V) into the folder named, else the current one (disabled in an album and with nothing kept): after Cut `photo.moveTo`, which then empties it, after Copy the copy question |
| `albumProperties` | `null`, or `{album, ids, name, date, useNewest, place, description}`: the album edited (`null` for a new one, made with `ids`), the date as typed with "Use the date of the newest photo"; OK (enabled while the name is not blank) runs `album.create` or `album.info.set` | `album.new {ids?}` (File ▸ New Album…, Ctrl+N, the toolbar's New Album: the photos named, else the tray's), `section.properties` on an album section (Album ▸ Album Properties…) |
| `renameFiles` | `null`, or `{ids, name, addDate, addSize}`; Rename (enabled while the name is not blank) runs `photo.rename` | File ▸ Rename… and F2 (the selection) |
| `newFolder` | `null`, or `{ids, name, date, useOldest, place, description}` (name "New Folder"); OK runs `photo.moveToNewFolder` | File ▸ Move to New Folder… (the selection, photos of one folder), the photo menu's Split Folder Here… (`photo.splitFolder {id?}`: the photo and every photo after it in its folder) |
| `moveFiles`, `copyFiles` | `null`, or `{ids, folder, dontAsk}` / `{ids, folder}` while the move or copy question is open | `photo.moveTo` without `confirm` (§5.5), Paste after Copy |
| `collision` | `null`, or `{op: "move" \| "copy", ids, folder, files}`: the clash and the names that clash (at most 1000) | a move or copy onto files of the same names (M9.19) |
| `deletePhotos`, `deleteFolder` | `null`, or `{ids}` / `{folder}` while the delete question is open | `photo.delete`, `folder.delete` without `confirm` (§5.5) |

The `…` items whose engine command takes input (File ▸ Rename… and Batch Edit ▸ Rename…, F2, File ▸
Move to New Folder…, Folder ▸ Move…) open their dialog when clicked, pressed or invoked with
`ui.menu.invoke` (`{dialog: "renameFiles"}`, `{dialog: "newFolder"}`, `{dialog: "filePicker"}`: the
folder picker for where the folder goes, `filePicker {command: "folder.move", kind: "folder"}`),
and the dialog's default button runs the engine command with the input; `engine.execute` runs the
engine command itself (`photo.rename {ids, name}`), never the dialog. The Delete key is routed by
section, as Ctrl+1 is by view: in an album section it runs `album.remove {album, ids}` (Remove from
Album), anywhere else `photo.delete`; Ctrl+Delete always runs `photo.delete`.

A draft is non-null while its tool is open; nothing in it is applied before the tool's Apply
(`edit.tool.apply`, one step of the photo's history). `ui.set` merges `edit` field by field
(`{"edit": {"zoom": 2}}` keeps the photo and the tab); every other field is replaced whole. The
tools take their actions by name through `edit.tool.action {name, params?}` (each tool lists its
own); keys reach the open tool first, then Esc cancels it, then the edit view's keymap.

The dialogs M6 adds keep their drafts in `dialogs[].fields` like the others; each is opened by a UI
command and its default button runs one engine command (one undo step): `folderProperties`
(`section.properties {folder?}`: `{folder, name, date, useOldest, place, description}`, OK runs
`folder.info.set`), `adjustDate` (`photo.adjustDate {ids?}`: `{ids, current, mode: "shift" |
"set", shift, set}`, OK runs `photo.dates.set`), `quickTags` (`quicktags.configure`: `{useRecent,
slots}`, OK runs `tags.quick.set`), `renameTag` (`tag.rename {tag}`: `{from, to}`, OK runs
`tags.rename`), `about` (`app.about`, no draft: its state is `about` above) and `releaseNotes`
(`help.releaseNotes`); About and Release Notes have one button, Close (`button:dialog.cancel`, Esc).

`thumbSize`, `treeMode`, `folderSort`, `folderSortReverse`, `caption`, `smallPictures` and
`hiddenPictures` are kept in the library's `settings.json` (with the tree width and the side pane)
when the app runs in a window; `collapsedGroups`, `collapsedFolders`, the selection, the tray and
the scroll anchor start fresh. `ui.set` takes any subset of these fields, validates all of them
first (unknown fields and bad values are `bad_params`, ids that do not exist `not_found`; nothing
changes) and returns the new state; repeated ids in a list are kept once. Prefer the equivalent
command (`view.edit`, `pane.tags`) when there is one: commands are what users trigger and what
parity tracks.

### 6.6 `ui.inspect` (with perf counters)

Everything in `ui.state`, plus:

| Field | Content |
|---|---|
| `window` | `{width, height, pixelsPerPoint, focused, drawing}` — `drawing` false while the compositor delivers no frames |
| `regions` | `[x, y, w, h]` per region of `ui-layout.md` §2: `menuBar`, `toolbar`, `tabStrip`, `tree`, `splitter`, `banner` (the filter banner, in search mode only: the 32 rows over the light table and its scroll bar, which start under it), `lightTable`, `scrollbar`, `sidePane`, `statusLine`, `tray`, `trayButtons`, `bottomControls`, `actionButtons`; in the edit view (`editing.md` §2) `menuBar`, `editTopBar`, `filmstrip` (inside the top bar), `editPanel` (`null` while hidden), `canvas` (the photo area above the caption bar), `captionBar`, `sidePane` and the bottom area's four, while the library's own are `null`; `null` when not shown |
| `scroll`, `scrollViews` | `scroll` is `ui.state`'s anchor (`{section, offset}`); `scrollViews` is `{lightTable, tree, topSection, sticky}` — the light table's and the tree's offsets in points as last drawn, the section at the top, the pinned header (section ids as in `ui.state`, `null` for none) |
| `hovered` / `focused` | widget ids |
| `popup` | the open menu, context menu or popup as last drawn, `null` when none: `{kind, menu?, items: [{widget, id, label, enabled, checked}]}`; `kind` is `menu` (with `menu`, the bar menu's key: `File`), `viewOptions` (the toolbar's popup) or `contextMenu`; `id` is the row's command (`null` for a submenu row) |
| `dialog` | the top dialog, `null` when none: `{kind, widget, title, fields: {name: value}, buttons: [widget ids], default, command?}` (`default` the widget of the button Enter presses) |
| `filePicker` | the stubbed picker waiting for `ui.filePicker` (§5.6): `{command, kind, title}`, and for the save picker `name`, `folder` and `params` |
| `status`, `toasts` | the status-line text as drawn; the toasts showing, oldest first: `[{id, text, kind, remaining}]` (`id` the toast's widget `toast:<n>`, `kind` `info` or `error`, `remaining` in seconds) |
| `tooltip` | the text of the tooltip the last frame showed (its lines joined with `\n`), `null` when none |
| `drag` | a drag of photos in progress, `null` when none: `{from, ids, target, op, pill}`: the widget it started on (`thumb:12`, `thumb:12/in/a91c0e`, `tray:item:12`, `tray:token`), the photos it carries (the selection when the photo was selected, else that photo; from the tray the tray-selected photos when the mini is tray-selected, else that one), the drop target under the pointer (a widget id: `tree:folder:<id>`, `tree:album:<id>`, `tray`, or for the gap beside a cell of a user album the cell's `thumb:<id>/in/<album>`; `null` for none, and where a drop would change nothing: the photos' own folder, an album or a tray already holding them all, the gap beside a dragged photo), what a drop there would do (`move`, `copy`, `add`, `reorder`, `hold`, `null`) and the drag pill's text ("Move 3 photos to Garden", "Copy 1 photo to Garden", "Add 3 photos to Lisbon", "Hold 3 photos in the tray", "Reorder 3 photos", "3 photos" over nothing); it ends when the button is released (dropping there) or Esc is pressed (dropping nothing). Not part of `ui.state` |
| `animations` | running animations: `[{kind, region}]`, today `{kind: "scrollToSection", region: "lightTable"}` while the light table eases to a section (pane slides and fades join as they are built) |
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
| `coveredCells`, `upscaledCells`, `sketchCells`, `placeholderCells`, `failedCells` | light-table cells last frame by what they drew: the covering ladder size, a smaller one scaled up, the colour sketch, the `gray 38` placeholder (never scanned; 0 while scrolling a scanned library), a failed thumbnail (`architecture.md` §6.5) |
| `thumbTextures`, `thumbTextureBytes`, `thumbUploads`, `thumbUploadBytes`, `thumbUploadsDeferred`, `thumbEvictions` | thumbnail textures alive (at most 600, kept within one screen of the view), uploaded last frame (at most 24 and 4 MiB a frame), left for a later frame by that cap, evicted last frame |
| `fsCachedPending` | file-system answers the UI is waiting for (`fs_cached`) |
| `opsApplied` | scanner `Op`s applied last frame |
| `previewDraftMs`, `previewFinalMs`, `previewRefinedMs` | the edit view's preview (M7.15): the latest draft, final preview and refined window of a zoomed view in the window of frames, each from its request to the frame that showed it (`null` when none arrived) |
| `previewWaitingFrames` | frames in the window that drew the scaled preview while the refined window of a zoomed view was still on its way (they did not wait for it) |
| `memory` | `{budgetBytes, usedBytes, byCache: {…}}` (`architecture.md` §5) |
| `gpu`, `gpuFallback` | adapter in use; latest CPU fallback and its reason (`development.md`) |

### 6.7 `ui.widgets`

```json
[{"id": "tree:folder:1", "rect": [0, 128, 240, 24], "enabled": true, "label": "Lisbon Weekend (48)", "role": "treeRow"},
 {"id": "thumb:3", "rect": [282, 230, 144, 108], "enabled": true, "role": "thumbnail", "checked": true}]
```

Widgets drawn in the **last frame**, in paint order, with screen-point rects (clipped to what is
visible). `filter` is a substring of the id; `region` restricts to a region of §6.6 (the widgets whose
centre is inside it, with its `region:<name>` entry; none for a region not shown). `checked` is the
selected/toggled state where one exists. Off-screen thumbnails are not listed: scroll first
(`ui.clickWidget` on the tree row, or `ui.scroll`). Ids are in §7.

### 6.8 Clicks, drags, hover and `ui.pointer`

- `ui.clickWidget {id}` clicks the widget's centre; `fx`, `fy` (0..1) pick another point inside it;
  `count: 2` double-clicks; `button: "right"` opens context menus; modifier flags apply to the click
  (`ctrl: true` toggles a thumbnail in the selection, `shift: true` extends it).
- `ui.dragWidget {id, toWidget}` drags from one widget onto another — e.g. `thumb:5` onto
  `tree:folder:2` moves the photo (hold `ctrl: true` to copy), onto `tray` holds it, onto
  `tree:album:a91c0e` adds it to the album; `toX`/`toY` on the left or right half of an album
  cell `thumb:7/in/a91c0e` puts it before or after that photo in the album. `toX`/`toY` (screen points) or `dx`/`dy` give a point
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
context menu's model for the current selection without opening it. `tray` is the tray menu as it
opens on the tray's empty space: for the tray-selected photos, or with none every photo in the tray.
`photo` is the thumbnail's menu (`organizing.md` §9.1), which a right-click on a thumbnail opens
after selecting the photo when it was not; in an album section Delete from Disk… becomes Remove from
Album and Show in Folder and Set as Album Cover join. On an album section the fourth menu reads
**Album** with the album rows of `organizing.md` §9.2 (Album Properties…, Delete Album…, Select
All, Select None, Invert Selection, Refresh Thumbnails, Sort Album By ▸), and `section` returns the
same rows.

Each item carries `id`, `label` (as shown: Undo names the action it undoes), `enabled`, `live`
(its command is registered, and for an item ending in `…` its dialog exists; an item whose `…`
dialog collects the input of an engine command, Rename… for one, is live with its dialog), `widget`, and when
they apply `shortcut` (the binding in effect, `Cmd` on macOS), `params` (the fixed parameters of a
row that shares its command), `checked` (checkable rows; radio rows add `radio: true`), `dialog`
(the dialog the item opens) and `reason` (why it is disabled). A submenu is `{label, widget,
children}`. Context menus are modelled as their regions arrive: `section` (the fourth menu's rows),
`tray` and `photo` answer now, the others are `unsupported` until then.

`ui.menu.invoke {path: "Picture/Batch Edit/Rotate Clockwise"}` (labels as shown, without a
trailing `…`, separated by `/`; a literal `/` in a label is `\/`) or `{id: "photo.rotate.cw"}`
behaves like clicking the item: items whose label ends in `…` open their dialog (reply
`{dialog: kind}`), others run their command (reply = the command's result, with `wait` as in
§5.3). Disabled items fail with `disabled`; catalog items that are not live yet fail with
`unsupported`. A command runs as `engine.execute` runs it (§5.5–§5.7 apply), with the
selection as `ids` when it takes them, the row's fixed parameters, then `params`. File ▸ Add
Folder… and Tools ▸ Folder Manager… both reply `{dialog: "folderManager"}` once a frame has drawn
it; Folder ▸ Remove from Dalica… replies `{dialog: "removeFolder"}`, asking about the selected folder. A command that
asks first (§5.5) opens its question and replies `{dialog: <kind>}`. Context-menu rows are driven
by clicking them (`ui.clickWidget {id: "menuitem:photo.splitFolder"}` once the menu is open).

Dialogs are driven through their widgets (`field:dialog.<name>`, `button:dialog.<action>`) or the
shortcuts `ui.dialog.confirm` (the default button, or `{button: "dialog.undoSave"}`, with or without
the `button:` prefix) and `ui.dialog.cancel`. Both answer the top dialog, as its buttons do: Cancel
closes it and changes nothing (`{}`); another button runs the dialog's action and replies its
result, once a frame has drawn the result. The dialog closes when the action succeeds and stays
open when it fails (the error is the reply). With no dialog open both are `not_found`; a button the
dialog does not have is `not_found`, a disabled one `disabled`. While a dialog is open the window
under it takes no input: its backdrop takes the clicks, and no shortcut runs and no panel sees a key
(Esc presses Cancel, Enter the default button, unless a field keeps the focus).

### 6.11 `ui.screenshot`

```text
→ {"id": 12, "method": "ui.screenshot", "params": {"path": "/tmp/after.png"}}
← {"id": 12, "ok": true, "result": {"path": "/tmp/after.png", "width": 1400, "height": 900, "headless": false}}
```

- Captures the whole window as a PNG at physical pixels, **after a settle** (§6.12, up to 5 s) so
  visible thumbnails and previews are finished; `settle: false` captures the next frame as is.
- `widget` or `region` (a region name or `[x, y, w, h]` in points) crops the capture; a region
  by name is the rect `ui.inspect` → `regions` reports (`not_found` while it is not shown).
- Without `path` the PNG comes back base64-encoded in `png` (subject to the 8 MiB reply limit).
  `path` follows §5.7 and must end in `.png`.
- `headless: true` draws the UI on the CPU (§8) instead of reading back the window: it needs no
  compositor and works when the window is occluded, minimised or the display is asleep.
  **Planned, not built yet:** in the running window it would need a second copy of the window's
  textures, so today the window reads its own frame back whatever `headless` says, and a window that
  draws no frames (minimised, covered) answers `timeout` within **2 s** (every input request does;
  on Wayland a minimised window gets no logic ticks either, so the 70 s request limit answers).
  An `--headless` app and `dalica-cli snapshot` always draw on the CPU (`headless: true` in the
  result).
- Dalica **never raises or focuses its window** to take a screenshot: stealing focus from the user's
  terminal is as bad as synthetic OS input.

### 6.12 `ui.settle` and `ui.wait`

`ui.settle {timeoutMs: 5000}` runs frames until nothing visible is pending for 12 consecutive
frames: no queued synthetic input, no high-priority (`editMain`, `hover`, `visible`, `filmstrip`)
jobs in flight, no running animation, no pending `fs_cached` answers, no scanner `Op` batch waiting,
no open file dialog (Add Folder's, the Save As picker, a folder picker such as the Export dialog's
Browse…) whose answer has not run yet. Background thumbnail generation and long jobs are not waited
for unless `jobs: true`: then also every metadata write still to land and every file job (Save,
Undo Save, Revert, Save a Copy, Save As, Export, file operations) until its results are in the
library (`busy`: `"file jobs: Exporting 12 of 48"`). Reply:
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
(and `finishedMs`) and stay listed for 10 minutes (at most 64 entries). `jobs.wait` lets frames run
while it waits (it never blocks one) and replies the final entry, or after `timeoutMs` (default and
at most 55 s; over 60 000 is `limit`) the running entry with `pending: true`; an unknown or expired
job is `not_found`. `jobs.cancel` replies once a frame has polled the session; a finished job is not
in `cancelled`. Kinds: `scan`, `import`, `export`, `save`, `files` (a move, copy, rename or delete of files, M9),
`batchEdit`, `metadataWrite`, `rebuild`, `duplicates`, `faces`, `collage`, `analysis`. `jobs.cancel` stops a job at its next photo boundary: photos already written stay
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
| Regions (M3) | `region:<name>` for each region of §6.6 shown (`region:menuBar`, `region:toolbar`, `region:tree`, `region:splitter`, `region:banner` (M9, search mode), `region:lightTable`, `region:scrollbar`, `region:sidePane`, `region:statusLine`, `region:tray`, `region:trayButtons`, `region:bottomControls`, `region:actionButtons`): the region's rect, no input of its own (role `other`); a region not shown is not listed |
| Menu bar (M3) | `menu:File` … `menu:Help` (title by its English label; the fourth is `menu:Folder` whatever its shown title; macOS adds `menu:Dalica`), `menuitem:<command id>` for open menu and context-menu items (`menuitem:<command id>/<variant>` for rows of one command that differ by a parameter, `menus-and-shortcuts.md` §1; M9: the Add to Album rows of the photo menu and of the tray's Add to Album ▾, one per user album, are `menuitem:album.add/<AlbumId>`), `submenu:<menu>/<label>…` (`submenu:View/Thumbnail Caption`), and on a menu taller than the window its scroll arrows `button:menu.scrollUp/<depth>` and `button:menu.scrollDown/<depth>` (depth 1 for a menu of the bar or a context menu) |
| Title bar (M3) | Windows and Linux, unless the system title bar is chosen (`design-system.md` §11.1): `button:window.minimize`, `button:window.maximize` (labelled Restore while maximized), `button:window.close`, `titlebar:drag` (the free gap: drag moves, double-click maximizes; also on macOS, beside the traffic lights); in a real window, while neither maximized nor full screen, the resize edges `resize:n`, `resize:s`, `resize:e`, `resize:w`, `resize:ne`, `resize:nw`, `resize:se`, `resize:sw` |
| Toolbar (M3) | `button:toolbar.import`, `button:toolbar.newAlbum`, `button:tree.flat`, `button:tree.tree`, `button:toolbar.viewOptions`, `button:filter.starred`, `button:filter.faces`, `button:filter.videos`, `button:filter.geotagged`, `slider:age`, `field:search`, `button:search.clear`, `indicator:activity`; M9: the search field's suggestions while listed, `field:search/option/<n>` (role `suggestion`, from 0 at the top, active while highlighted) |
| Room tabs (M11+) | `tab:room.library`, `tab:room.import`, `tab:room.collage`, …; `button:room.close.<room>` |
| Library tree (M4) | `tree:group:albums` · `people` · `projects` · `folders` · `other` · `hidden` (the group header), `tree:folder:<FolderId>`, `tree:album:<AlbumId>` (also `tree:album:starred`, `tree:album:recent`, `tree:album:search`), `tree:person:<PersonId>` (M13), `tree:disclosure:<FolderId>` (tree mode); in tree mode a directory between a watched root and its folders that is no library folder itself is `tree:dir:<hash>` (sixteen lowercase hex digits of the 64-bit FNV-1a hash of its path's UTF-8 text, stable for the path) and its triangle `tree:disclosure:dir:<hash>` (such directories fold for the session only: `collapsedFolders` holds folder ids); `splitter:tree` |
| Light table (M4) | `section:<section id>` (the header block, e.g. `section:folder:12`), its parts `section:folder:12/icon`, `/title`, `/description`, `/play`, `/collage`, `/selectStarred`, `/save`, `/export`, and with unsaved edits `/saveEdited` (the Save text button at the top right); while a header is pinned, its pinned line carries `section:<id>`, `/icon` and `/title` (and `/saveEdited`), and the block under it keeps only the ids of the parts shown below that line; `thumb:<PhotoId>` and its star badge `thumb:<PhotoId>/star` (a photo's cell in its folder's section); in an album's section the same photo's cell is `thumb:<PhotoId>/in/<album>` (`<album>` an album id, `starred`, `recent` or `search`; M9), its badge `thumb:<PhotoId>/in/<album>/star`, and a click there selects the photo in the album; `lightTable` (empty space: rubber band, section menu); with no watched folders the empty state `empty:lightTable` and its `button:empty.addFolder`, `button:empty.import`; `banner:search` and `button:search.showAll` (M9), and the empty state of a search with no match `empty:search` (M9) |
| Scrollbar (M4) | `scroll:prevSection`, `scroll:up`, `scroll:track`, `scroll:thumb`, `scroll:down`, `scroll:nextSection` |
| Status line (M4) | `text:status`, `indicator:unsaved`, `indicator:temporarySession` |
| Photo Tray (M3: the box and its buttons, disabled; M4: its contents) | `tray` (the box), `tray:token`, `tray:item:<PhotoId>` (its held badge `tray:item:<PhotoId>/held`), `tray:more` (the "+N" place), `button:tray.hold`, `button:tray.clear`, `button:tray.addToAlbum` |
| Bottom controls (M3) | `button:star`, `button:rotate.ccw`, `button:rotate.cw`, `button:loupe`, `slider:thumbSize`, `toggle:pane.people`, `toggle:pane.places`, `toggle:pane.tags`, `toggle:pane.properties`; edit view: `button:zoom.fit`, `button:zoom.1to1`, `slider:zoom` |
| Action buttons (M3) | `action:export`, `action:collage`, `action:slideshow`, `action:more`; below 1100 wide only `action:more` is on the row, and the rows of its open menu carry the actions' own ids (role `menuItem`) |
| Side pane (M6) | `pane:tags`, `pane:people`, `pane:places`, `pane:properties` (the pane body), `button:pane.widen`, `button:pane.close`; Tags: `field:tags.add`, `button:tags.add`, `tag:<tag text>` and its remove button `tag:<tag text>/remove` (shown on the hovered row), `quicktag:1` … `quicktag:10`, `button:quicktags.configure`; Properties: `button:properties.showAll` (shown while something is selected) |
| Inline editors and About (M6) | the caption editor under a cell `field:caption.inline`; a header's description editor `field:section.description`; the About dialog's tabs `tab:about.about`, `tab:about.contributors`, `tab:about.models`; on Contributors the name toggle `choice:dialog.names/<username\|displayName\|realName>`, the list toggle `choice:dialog.list/<grabBag\|table>`, the sort dropdown `choice:dialog.sort` with its rows `choice:dialog.sort/<sort>`, `button:dialog.reverse`, and the table's headers `button:dialog.sort/<sort>` (`<sort>`: `name`, `prs`, `commits`, `linesAdded`, `linesDeleted`, `linesDelta`, `binaryAdded`, `binaryDeleted`, `firstCommit`, `lastCommit`) |
| Edit view (M7) | regions `region:editTopBar`, `region:editPanel`, `region:canvas`, `region:filmstrip`, `region:captionBar`; `button:edit.back`, `button:edit.play`, `button:compare.one`, `button:compare.two`, `button:compare.same`, `canvas:edit` (the photo area: click, double-click, press and hold, drag to pan), `text:zoom` (the "Zoomed to 200 %" read-out, its text as the label, while zoomed), `navigator:edit` (the navigator while the zoomed photo is larger than the area: a click or drag centres the view there), `film:<PhotoId>`, `button:film.previous`, `button:film.next`, `field:caption`, `button:caption.toggle`, `button:edit.trash`, `tab:edit.basic`, `tab:edit.tuning`, `tab:edit.effects`, `tab:edit.effects2`, `tab:edit.effects3`, `button:edit.undo`, `button:edit.redo`, `button:edit.showOriginal`, `box:histogram` with its histogram `histogram:edit` and camera columns `box:histogram/info`, the effect tabs' empty state `empty:edit.effects` |
| Edit tools (M7) | `tool:crop`, `tool:straighten`, `tool:autofix`, `tool:autocontrast`, `tool:autocolor`, `tool:retouch`; `slider:basic.fill_light`; tool panels: `slider:straighten.angle`, `choice:crop.ratio` (its rows `choice:crop.ratio/manual`, `/current`, `/<W:H>` such as `/4:3`, `/custom/<name>` and `/add`), `button:crop.removeRatio`, `button:crop.suggestion/1` … `/3`, `button:crop.rotate`, `button:crop.preview`, the rectangle on the photo `crop:edit`, Retouch's `slider:retouch.brush`, `button:retouch.undoPatch` and `button:retouch.redoPatch`, `button:panel.apply`, `button:panel.cancel`, `button:panel.reset` |
| Tuning (M7) | `slider:tuning.fill_light`, `slider:tuning.highlights`, `slider:tuning.shadows`, `slider:tuning.temperature`, `button:tuning.autoLighting` (the Auto lighting wand), `button:tuning.neutralPicker` (the eyedropper, active while armed), `button:tuning.autoColor` (the Auto color wand), `button:tuning.reset` (left out when the tab is too short for it) |
| Effects (M12) | `effect:<name>` per tile (`effect:sharpen`, `effect:sepia`, `effect:bw`, `effect:soft_focus`, `effect:graduated_tint`, …); effect panels: `slider:effect.<param>` (`slider:effect.amount`, `slider:effect.fade`), `color:effect.<param>`, `check:effect.<param>`, `crosshair:effect` |
| Dialogs (M3+) | `dialog:<kind>` (the dialog: `dialog:clearTray`, `dialog:folderManager`, `dialog:firstRun`; M6: `dialog:folderProperties`, `dialog:adjustDate`, `dialog:quickTags`, `dialog:renameTag`, `dialog:about`, `dialog:releaseNotes`; M7: `dialog:customRatio` with `field:dialog.width`, `field:dialog.height`, `field:dialog.name`; M8: `dialog:saveExplain` with `button:dialog.save`, `dialog:revert` with `button:dialog.revert` and `button:dialog.undoSave`, `dialog:export` with `button:dialog.export` and its form (M8.20): `field:dialog.location`, `button:dialog.browse`, `field:dialog.folderName`, `check:dialog.numbers`, `choice:dialog.size/<original\|resize>`, the size field `slider:dialog.size/value` (its text as typed) and the slider `slider:dialog.size`, the quality dropdown `choice:dialog.quality` with its items `choice:dialog.quality/<automatic\|normal\|maximum\|minimum\|custom>` and the custom slider `slider:dialog.quality`, `choice:dialog.videos/<firstFrame\|fullVideo>` (disabled until M15), `check:dialog.watermark`, `field:dialog.watermarkText`, and the export progress window `dialog:exportProgress`, which is no dialog of the stack and has no backdrop; M9: `dialog:albumProperties` and `dialog:newFolder` with `button:dialog.ok`, `dialog:renameFiles` with `field:dialog.name`, `check:dialog.addDate`, `check:dialog.addSize` and `button:dialog.rename`, `dialog:moveFiles` with `button:dialog.move` and `check:dialog.dontAsk`, `dialog:copyFiles` with `button:dialog.copy`, `dialog:collision` with `button:dialog.renameDuplicates` and `button:dialog.skipDuplicates`, `dialog:removeFromAlbum` with `button:dialog.remove`, `dialog:deleteAlbum` with `button:dialog.deleteAlbum`, `dialog:deletePhotos` and `dialog:deleteFolder` with `button:dialog.moveToTrash`), `dialog:<kind>/backdrop`, `field:dialog.<name>`, `check:dialog.<name>`, `choice:dialog.<name>`, `button:dialog.ok`, `button:dialog.cancel`, `button:dialog.<action>` (`button:dialog.undoSave`, `button:dialog.clearTray`, `button:dialog.help`, `button:dialog.continue`); M6's dialogs: Folder Properties `field:dialog.name` (renames the folder on disk, M9.20), `field:dialog.date`, `check:dialog.useOldest`, `field:dialog.place`, `field:dialog.description`, `field:dialog.music` and `button:dialog.browse` (disabled until slideshows); Adjust Date `field:dialog.date`, `field:dialog.time`, `choice:dialog.mode/<shift\|set>` (several photos); Rename Tag `field:dialog.to`; Configure Quick Tags `check:dialog.useRecent`, `field:dialog.slot1` … `field:dialog.slot10`, `button:dialog.fillCommon` |
| Folder Manager and first run (M5) | rows `choice:dialog.folder/<roothash>[/<pathhash>]`, `button:dialog.expand/…` (and `…/loading` while a listing is read), `choice:dialog.mode/<always\|once\|remove>`, `check:dialog.faces`, `button:dialog.reveal/<hash>` (the watched list), `choice:dialog.start/<places\|choose>` (first run); stacked questions `dialog:removeFolder`, `dialog:watchDrive`, whose buttons are `dialog:<kind>/<action>` while another dialog is under them |
| Activity and tree scroll bar (M5) | `indicator:activity` popover `popover:activity`, its rows `job:<n>` and `job:<n>/cancel`; the tree's `scroll:tree`, `scroll:tree/thumb` |
| Toasts (M3) | `toast:<n>`, `button:toast.<action>` (`button:toast.showFolder`) |
| Drag and drop (M9) | `drag:pill` (the drag pill while photos are dragged: role `other`, painted, labelled with its text) |

Rules for implementers:

- Register through the one helper in `widgets.rs` (never by hand); a widget that is drawn but not
  registered is a bug caught by the headless "every interactive widget has an id" test.
- An id is unique within a frame. Repeated widgets carry their model id (`thumb:<PhotoId>`), never a
  position index; a model object shown twice carries where it is shown (`thumb:<PhotoId>/in/<album>`).
- New regions add their ids to this table in the same commit.

## 8. Headless rendering (no window, no GPU)

The egui output is rasterized on the CPU (`crates/ui-kit/src/raster.rs`, driven by
`crates/ui/src/headless.rs`; our own, modelled on LightCraft's): same tessellation, gamma-space premultiplied
blending and scissor clipping as egui's GPU backends, so the image matches the window (minus GPU
dithering). Thumbnails and previews come from the same job pool, drained while settling.

- **In the running app (planned):** `ui.screenshot {headless: true}` would draw the current UI into
  an offscreen context from the logic tick; until then a window that draws no frames answers
  `timeout` after 2 s (§6.11). `--headless` runs the app on this renderer from the start.
- **Without any window:** `dalica-cli snapshot` (`apps/dalica-cli/src/snapshot.rs`) opens a
  library, drives the real UI through the headless driver (below), waits for it to settle, paints
  the last frame and writes one PNG. It does not go through the control handler: no TCP, no token,
  and its script is a list of driver steps, not control requests.

```sh
dalica-cli snapshot --demo -o grid.png --size 1400x900
dalica-cli snapshot --demo-fresh --script tour.jsonl -o out.png --size 1400x900 [--scale 2]
dalica-cli snapshot --library DIR --script tour.jsonl -o shot.png
```

- **Library:** `--library DIR` (required, or `--demo` / `--demo-fresh` in its place, which
  `dalica-cli` turns into `--library <the demo's library>` after making and scanning it). The
  library is opened like any other, with its own settings: commands in the script change it, and
  what they changed is flushed before the command exits (a failed flush exits 1). `--demo` is the
  persistent demo library (§1), so a script's stars stay in it; `--demo-fresh` writes the
  procedural photos into a new temporary folder removed on exit, the clean slate a repeatable
  script wants. Neither touches the user's own library, settings or window state.
- **Script format** (`--script FILE`, ≤ 16 MiB, UTF-8): one JSON object per line, one of
  `{"click": widgetId}` (§7), `{"key": "Ctrl+8"}`, `{"command": id, "params": {...}}` (an engine
  or UI command, run as a menu item would; `params` defaults to `{}`) or `{"settle": true}`; blank
  lines and lines starting with `#` are skipped. A line that is not a JSON object is a usage error
  (exit 2) before anything runs.
- **Order and failure:** the library settles first, then the lines run in order, then it settles
  again (each settle waits up to 18,000 frames for nothing visible to be pending, §6.12). The first
  failing step (an unknown step, a missing or disabled widget, a refused command, a settle that
  times out) stops the script: its reason goes to standard error with its line number, no PNG is
  written and the exit status is **1**. Usage errors (a missing `--library` or `-o`, a bad `--size`
  or `--scale`, an unreadable script) exit **2**; a library that cannot be opened exits **3**.
- **Output:** on success the PNG is written atomically to `-o` (replacing a file there) and one
  JSON line goes to standard output: `{"ok": true, "path", "width", "height", "scale", "frames",
  "ms"}`.
- **Size:** `--size WxH` is the image in **pixels**, each side 1 … 8,192 (default 1400x900);
  `--scale` is pixels per point, 0.5 … 4 (default 1), so `--size 1400x900 --scale 2` lays out a
  700 × 450-point window. Both are checked before anything is opened.
- **Clock:** the driver's frames advance a virtual clock (1/60 s per frame), so animations and
  settling do not depend on the machine's speed; the session itself reads the system clock, so a
  view that shows "today" or relative dates follows the real date.

**Not built yet** (promised here before; planned, no task yet): folders named on the command line
(scanned once, in memory); a read-only `--library`; control requests in scripts
(`{"method", "params"}` with a JSON reply per line on standard output, a failed line not stopping
the rest); several `ui.screenshot`s per script; a fixed "now" (`--now`); the working directory and
the directory of `-o` as write roots for `path` params (§5.7); `cargo xtask snapshot` (listed as
planned by `cargo xtask`); a snapshot row in `cargo xtask bench`.

**Rust test driver:** headless UI tests use the same driver directly (`crates/ui/src/headless.rs`):
`Driver::new(app, width, height, pixels_per_point)`, `click(id)`, `key(chord)`, `run(id, params)`,
`settle(max_frames)` (or `settle_for(max_frames, jobs)` to wait for long jobs too),
`step_until(done, max_frames)`, `wait(ms)` and `screenshot()`. Assert on `library.inspect` /
`ui.inspect` and perf counters, not on pixel counts, except in fingerprint tests
(`architecture.md` §11).

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

Planned, not built yet: `dalica-cli run --connect 127.0.0.1:7990 --script FILE.jsonl` (these
request lines, token read from the default token file; `--keep-going` to continue after a
failure). Until then use a client like the one above.

### 9.2 Tour: select across folders, hold, edit, look

```jsonl
# tour.jsonl — demo library, sent to `dalica --control … --demo` (a `dalica-cli snapshot` script
# takes the step lines of §8 instead)
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
  without the maintainer asking; `dalica-cli snapshot --library DIR` is **not** read-only (§8), so
  look at a copy of the library, not the library itself.
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
