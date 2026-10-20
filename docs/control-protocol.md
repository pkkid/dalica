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
| `--automation-allow-people` | — | Let people's names and nicknames and face rectangles reach automation, and its searches match names (§6.4). Off by default; a flag only, needing `--control`. |
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
| `read_only` | a read-only session (`--read-only`) refuses what would write files: file operations, saves and exports | open |
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
  as in the UI. By default the reply waits for the job (frames keep running; none is blocked) and
  carries its result: the command's own fields, then the job's final entry as `jobs.wait` gives it
  (`job`, `kind`, `state`, `progress`, `result` or `error`, …; `pending` is dropped, a `scanning:
  true` becomes `false`). With `"wait": false` the reply is the command's own, with `job` and
  `pending: true`, at once. A job still running 55 s into the reply deadline replies
  `{…, job, pending: true}` too, so a long export never ends in a blind `timeout`. Follow up with
  `jobs.wait` / `jobs.list`. A command's job is the one it names itself (`export.run`, Save, the
  file operations answer `{job, pending: true}`); the scans (`folders.add`, `folders.set`,
  `library.rebuild`) name the scan or rebuild job they start. When a command starts several (a
  `folders.set` that rescans two roots), the reply waits for all of them and carries the first
  one's entry.
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
`external_denied`: Check for Updates, opening the help or release-notes web pages, and anything that would
use the network (map tiles, geocoding, model downloads; `AGENTS.md` "Privacy"). That includes
`options.set` changing an option that lets the app connect or decides where (turning
`network.allowMapTiles` on, `network.tileServer`, `network.geocoder`, `network.proxy`,
`network.proxyHost`, `network.proxyPort`: `automation::EXTERNAL_OPTIONS`; MCP refuses it alike),
and in the window the Places pane's Allow (`map.allowTiles {on: true}`), a place searched by name
(`map.search`) and the same changes on Options ▸ Network, which refuse with the reason on the
status line while a request drives them (M13.28).

**Some effects are never automated, whatever was granted** (M15.17, `security.md` §5.11): Locate
on Disk (`photo.locate`, `folder.locate`: opens the system's file manager), Open With
(`photo.openWith`: starts another program) and Open in External Editor (`photo.openExternal`) are
always refused over automation with `external_denied` (`engine.execute`, `batch`'s steps, MCP's
tools), **even with `--automation-allow-external`**, as is Remove from Source
(`import.removeFromSource`, which deletes files from a card); the message says why per command
(`automation::NEVER_AUTOMATED`, `automation::never_automated`). Only the person at the computer
runs them.

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

- **Reading** (adding a watched folder, `library.openFile`, import from a folder, a face model
  file: `faceModel.addFile {path}`, `faceModel.open {file}`, `faces.models.inspect {path}` and
  `faces.models.install {path}`): the path must be inside an `--automation-read-root` or an
  already watched folder. With no read root, adding new folders or reading a model file over
  automation is refused (`path_denied`), except in `--demo`. The window's own Add a Model File…
  picker and a `.onnx` file dropped on it are the user's and are not checked (the picker's answer
  to automation is, §5.6). The face model dialog's Add Model, pressed by automation, installs only
  the file the dialog opened on, and only while it lies (as written) inside a read root or a
  watched folder: a `faceModel` draft changed by `ui.set` after the dialog opened is refused.
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
- **An import's destination**: `import.run`'s `destination` (the folder an import makes its
  folders in) follows the same rule, checked first in the same way. When automation's input runs
  the Import room's buttons or keys instead (`engine.execute` of `import.importAll` /
  `import.importSelected`, a click on Import All or Import Selected, Enter, a pointer event, a
  batch step), the destination strip's folder is resolved through its links on a worker before
  the command or the input runs, and the app refuses the import (its reason on the status line) when it
  lies outside every write root and watched folder; a folder that changed while the input played
  was not checked, and is refused too.
- **A collage's folder**: `collage.create` and `collage.saveDraft` write into their `path`, else
  beside the collage the room reopened, else `<Pictures>/Dalica/Collages`; that folder follows the
  same rule, checked first. The Collage room's Create Collage, Save Draft and the close question's
  Save Draft write into the same folder: `engine.execute` of `collage.createCollage` /
  `collage.saveAsDraft` and `ui.dialog.confirm` of Save Draft answer `path_denied` before anything
  runs, and a click, key, pointer event or batch step that presses one is checked on a worker
  first, as the Import room's are, the refusal on the status line and nothing written.
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
and the status line shows a `caution` marker (widget `indicator:unsaved`). `pendingWrites` counts
every sidecar write not yet landed, failed or not: from the change, through the writer's one-second
debounce, until its file is committed (or waits for an offline volume); `null` means every change
is on disk.

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
`library.query`, …) change nothing and record no undo step. `wait` (default `true`) says whether
the reply waits for a job the command starts (§5.3); anything but a boolean is `bad_params`.

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
undo step — a batch is **not** a transaction. `stopOnError` defaults to `true`. The reply is
`{completed, failed, results}`, `results` holding every step's `{ok, result}` or
`{ok, code, error, prompt?}` in order; it stops early, with a final `limit` entry, when the aggregate
reply would exceed 8 MiB (the step that hit the budget ran; its own result is left out).

- The batch is checked before any step runs: more than 256 steps is `limit`; a step that is not an
  object with exactly one of `command` and `method` (and only its keys), or names `batch`, `auth`
  or `app.quit`, is `bad_params`. Either way nothing runs. A misspelt method in a step is that
  step's `unknown_method`, as a separate request's would be.
- The batch answers within the one reply deadline: the command steps share one wait for their
  jobs, which ends 55 s after the batch started (later steps then reply `{job, pending: true}` at
  once). A step that waits on its own (`ui.settle`, `jobs.wait`) still counts against the deadline.
- A key a step holds down (`ui.key {state: down}`) is let go when the batch ends: hold keys across
  requests with separate requests, whose connection lets go of them when it closes (§6.9).

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
  light-table order (a photo in an album and its folder is listed in both sections), paged by
  `offset` (at most 10⁹) / `limit` (default 200, at most 1 000), and `page: {offset, limit,
  total}`. `section` restricts the listing; `offset` and `limit` without a listing, a larger value
  or a section that is not a section id are `bad_params`, an unknown folder or album (or
  `album:search` outside search mode) `not_found`. `rotate` is the recipe's clockwise turn in
  degrees, `edited` whether the photo has unsaved edits, `recipe` its operation strings, `geo`
  whether its file holds a location, `date` its effective date; `held` and `selected` come from
  the window, so the engine query (headless MCP, the CLI) leaves them out.
- The engine query builds `tree` and `sections` from the stored view settings with every group
  expanded; the control method uses the window's own (its collapsed groups too). `counts.photos`
  counts every photo and video (`library.query`'s total), `videos` the videos among them, `hidden`
  the photos hidden by Hide or in a hidden folder. A section is `{section, name, count, shown, date,
  place, description, edited}`: `count` every photo it holds, `shown` those the view shows, `edited`
  those shown with unsaved edits. A year divider of the flat Folders group is `{divider: 2026}`;
  `year` on a folder row is its date's. `problems` entries are `{kind, text, path}` (`index`,
  `notice`, `unreadable`, `scan`, `offline`; at most 1 000), and `fileOpRepairs` lists the repairs
  of interrupted file operations. `current` is the section the top of the light table is in,
  `visibleSections` those on screen as last drawn, `selection.focus` the anchor. Before the
  library has opened the method is `disabled`.
- `search` is search mode (`organizing.md` §8, `ui-layout.md` §9): `active` and `mode` (`"search"`,
  `"duplicates"` for Find Duplicates' groups, `null` while not searching); the query, `age` in days
  (0: every age; the toolbar's slider is a position in `ui.state` → `search.age`, turned into days
  on its logarithmic scale); and once the worker's answer came the banner's numbers: `matches`
  (`null` before it and outside search mode), `folders`, `bytes`, `seconds`, the `sections` that
  keep a match with their counts (the search album first), `unavailable` (operators and filters
  that match nothing for want of data: `person` while the library has no people, `faces` while no
  photo has a face, `color` until colour search exists) and `truncated` (the text was cut at the
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
- **What is withheld, command by command** (M13.15; the engine's `automation::PEOPLE_REPLIES`
  and `Policy::people_reply`, one rule for the control channel and MCP): without
  `--automation-allow-people` the replies of `faces.list`, `faces.add` and `people.list` keep
  their ids (faces `"<PhotoId>.<n>"`, people `p…`), photos, counts and flags (`ignored`,
  `created`, `changed`), while every `name` and `nickname` in them is `null` and every face
  rectangle (`rect`, `displayed`) is left out. The commands themselves run as asked (naming a
  face by a person's id or by a name the caller gives is allowed: the caller already knows the
  name). With the permission the replies are as the engine gives them. Whatever was granted,
  `embedding`, `embeddings` and `landmarks` never appear in a reply, and no error message holds
  a name or a rectangle (people are named by id). A command that comes to answer with people
  data joins `PEOPLE_REPLIES`. Over the control channel `engine.execute` and each `batch` step
  answer through the policy the app's `--automation-allow-people` builds (M13.48), and without it
  `export.defaults` of a person's section suggests `fallbackName` as `folderName` (M13.49), as
  MCP's does.
- **No face points** (M13.49; `automation::FACE_POINTS`, the same `Policy::people_reply`, one rule
  for the control channel, headless MCP and connect mode): without `--automation-allow-people`
  `slideshow.list`'s `focus` gives each photo's focus as the photo's centre, `{"point": [0.5,
  0.5]}`, with no `face`, so no reply tells where a face is; the window's own show still moves
  toward the face. No reply carries a running show's focus (`ui.state`'s `slideshow.pan` is the
  viewer's own pan); one that comes to follows the same rule.
- **No names in the window's replies** (M13.49; `control/people.rs`): without
  `--automation-allow-people` `ui.inspect`, `ui.state`, `ui.widgets` and `ui.menu.tree` (also as
  `batch` steps) carry no person's name or nickname. The placeholder **Person** stands where a name
  was, the rest of a label kept ("Ana (12)" → "Person (12)", "Ana, 3 photos" → "Person, 3 photos",
  "Ana?" → "Person?"), and every widget id stays, so a client still finds, clicks and drags by id:
  the People group's rows (`tree:person:<id>`), a person section's header, `/icon` and `/title`,
  the People pane's rows (`person:<id>`), face tiles (`face:<face>`, `face:<face>/in/<id>`), the
  edit view's face boxes (`facebox:<face>`), the people the name fields offer
  (`field:faces.name/<n>/option/<k>`, `field:people.name/<face>/option/<k>`,
  `field:faces.manual/option/<k>`), the rows of Move to Person ▸ and Merge Into ▸
  (`menuitem:faces.moveToPerson/<id>`, `menuitem:people.merge/<id>`, in `ui.widgets`, `ui.inspect`'s
  `popup` and `ui.menu.tree`), the search banner while the People pane filters by people ("Showing
  photos with Person and Person"), a face drag's pill, the tooltip over any of these (a person
  section's icon says their name), the names typed (`unnamed.names`, `peoplePane.manual.name`), the
  Export draft's folder name while it exports a person's section (`export.folderName`; and the
  export job it starts, §6.13), and
  New Person's and Rename Person's drafts (`fields.name`, `fields.nickname`, in `dialogs` and
  `ui.inspect`'s `dialog`). `ui.menu.invoke {path}` matches a person's row by the placeholder only
  (by `id` and `params` as before). What is withheld is chosen by widget id, role and dialog kind,
  never by matching a reply's text against the library's names, which would change a folder or a
  caption that happens to read like a guessed name and so confirm the guess. Screenshots show
  what the window shows (§11). With the permission every reply is as the window has it.
- **No guess-oracle in searches** (M13.48, `faces.md` §7; `automation::NAME_SEARCHES` and
  `Policy::people_params`): without `--automation-allow-people` the searches an automation
  surface runs (`library.query`, `search.set`, `slideshow.list`'s `search`) go with
  `"withholdNames": true`, whatever the
  caller sent: people's names and nicknames take no part in the match, so `person:` matches
  nothing and is listed in `unavailable` (as in a library without people), plain words skip names
  and nicknames, and everything else matches as before. Nothing is refused, so no reply tells a
  guessed name from an unknown one. The exact filter by person id (`people: [PersonId]`, the
  photos holding a face of every one of them) stays allowed: ids are not names and `people.list`
  gives them. The switch belongs to one call: a `search.set` the window makes itself (typing in
  the search field, the People pane) always matches names. `library.inspect`'s `search` lists
  the People pane's filter as `people`. With `withholdNames` the folders a person's export made (§6.13) take no part
  in the match either: no word finds a photo through such a folder's name (M15.42).
- A person's section is `person:<PersonId>` (§5.4): `library.inspect`'s `photos` and `section`,
  `library.query`, `slideshow.list`, `export.*`, `places.list` and `section.refreshThumbs` take
  it (the photos holding the person's faces, by date like a folder); a person section listed here
  carries no `name` (names are `people.list`'s). `counts.people` counts the people.
- **The previous organizer's data** (M14.13, `data-model.md` §9): each folder row of `tree`
  carries `source`, the sidecar in effect: `"legacy"` or `"both"` as the scans found the previous
  organizer's file, else `"dalica"` for a folder the model says something about (what Dalica's own
  file holds, or will once written) and `"none"` for one it says nothing about (also a directory
  row that is no library folder); each listed photo carries `legacyEdits` (as `photo.inspect`'s
  entries do, M14.28), how many of its recipe's entries are kept as `legacy:` (operations Dalica has no equivalent for); and
  `legacy: {folders, notice}` counts the folders whose data came from a legacy sidecar (`legacy`
  or `both`) and gives the count the first-run notice waits to show (`null` once given, or while
  no scan found any). The engine query `legacy.inspect {folders?}` gives per folder (by default
  every folder with legacy data; an unknown one is `not_found`) its `source` and what the model
  holds there (`photos`, `starred`, `captioned`, `tagged`, `hidden`, `edited`, `legacyEdits`,
  `faces`, `geotagged`, and the distinct `albums` and `people` with a legacy id, counted, never
  named), with `readLegacyData` (the option), `notice`, `noticeShown` and how many `albums` and
  `people` carry a legacy id. `legacy.noticeShown {}` records the notice as answered
  (`files.legacyNoticeShown`). Legacy data comes in as a scan's imports do (no undo step) and a scan
  writes nothing into a legacy-only folder: its first `.dalica.json` comes with the user's first
  change there, after which its `source` is `both`. Options → General → "Read photo data left by
  the previous organizer" (`files.readLegacyData`, an `options.set` that rescans) off takes the
  data of legacy-only folders out of the model (and the albums and people only it named, once
  empty), on brings it back; neither writes into the photo folders.

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
          "crop": null, "straighten": null, "retouch": null, "tuning": null,
          "second": null, "focus": "left", "layout": "sideBySide", "showOriginal": false},
 "import": {"focus": null, "selection": [], "preview": null,
            "destination": {"place": null, "folderName": null, "after": "leave"}},
 "slideshow": null, "peek": null, "viewer": null, "language": "en", "status": "No selection", "dialogs": [], "activity": false,
 "captionEdit": null, "descriptionEdit": null, "propertiesShowAll": false,
 "tagsPane": {"draft": "", "highlight": null},
 "about": {"tab": "about", "list": "grabBag", "sort": "firstCommit", "ascending": true, "names": "username"},
 "export": null, "saveExplain": null, "revert": null, "exportProgress": null,
 "search": {"text": "", "highlight": null, "filters": {"starred": false, "faces": false, "videos": false, "geotagged": false},
            "age": 0, "savedScroll": null},
 "clipboard": null, "albumProperties": null, "renameFiles": null, "newFolder": null, "moveFiles": null,
 "copyFiles": null, "collision": null, "deletePhotos": null, "deleteFolder": null, "options": null,
 "unnamed": {"open": false, "expanded": false, "showIgnored": false, "selection": [], "names": {}},
 "people": {"sort": "name", "zoomToFaces": [], "onlySuggestions": []},
 "peoplePane": {"selected": [], "hovered": null, "manual": null},
 "places": {"centre": [0, 0], "zoom": 1, "pin": null, "bubble": null, "search": "", "results": [], "drop": null},
 "faceModel": null,
 "collage": {"tab": "settings", "selection": [], "clips": [], "gesture": null, "getMore": false, "close": null},
 "video": {"playing": false, "position": 0, "trim": null, "volume": 80, "fullScreen": false},
 "openWith": null, "dragOut": null, "defaultViewer": null}
```

`view` is `library | edit | slideshow | viewer`; `room` is the active tab (`library`,
`import`, `collage`) and `rooms` the tabs open (M11, below); `status` is the status line's text as drawn (read-only); `treeWidth` is
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
| `dialogs` | the open in-app dialogs, bottom first (the last takes input): `{kind, fields, command?}`, `kind` as in `dialog:<kind>` (`clearTray`, `folderManager`, `firstRun`, `removeFolder`, `watchDrive`; M6: `folderProperties`, `adjustDate`, `quickTags`, `renameTag`, `about`, `releaseNotes`; M7: `customRatio`; M8: `saveExplain`, `revert`, `export`; M9: `albumProperties`, `removeFromAlbum`, `deleteAlbum`, `moveFiles`, `copyFiles`, `collision`, `newFolder`, `renameFiles`, `deletePhotos`, `deleteFolder`; M10: `options`; M11: `keepVersion`, `applyChanges`, `undoAllEdits`, `removeFromCard`, `viewerSettings`; M13: `newPerson`, `ignoreFaces`, `renamePerson`, `mergePeople`, `deletePerson`, `faceModel`, `changeLocation`, `clearLocation`, `removeFaces`, `writePeopleNames`, `removeFaceModel`, `dropFaceData`; M14: `collageClose`; M15: `defaultViewer`), `fields` its draft (nothing is applied before its default button), `command` what a confirmation runs when confirmed | read-only here: the commands that open them (`folders.manager`, a `tray.clear` that asks), their buttons, `ui.dialog.confirm`, `ui.dialog.cancel` |
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
| `edit.tool` | `null`, or the open tool: `crop` \| `straighten` \| `retouch` \| `tuning` (a Tuning session) \| `effect` (an effect's parameter panel, M12; `edit.effect` says which) | `edit.tool.open {tool, name?}` (`{tool: "effect", name: "sharpen"}`), `edit.tool.apply`, `edit.tool.cancel` (Esc while a tool is open), leaving the edit view |
| `edit.compare` | `one` \| `two` (A│B) \| `same` (A│A) | `edit.compare {mode}`, the compare toggle (M11.16: `two` starts with the photo shown on the left and its section neighbour on the right, the right focused; `same` opens the photo's variant on the right; leaving A│A with sides that differ asks "Keep which version?") |
| `edit.zoom` | `"fit"`, or the scale of the photo's actual pixels, 0.001 … 4 (1 is 100 %; a scale at or below the fitted one shows fitted) | `edit.zoom.fit` and `edit.zoom.actual` (the bottom area's Fit │ 1:1, animated), `edit.zoom.in` / `edit.zoom.out` (Ctrl+= / Ctrl+−, ×1.25 about the pointer), `edit.zoom.set {slider}` (the zoom slider: 0 fit, 0.5 100 %, 1 400 %), the wheel over the photo (×1.1 a notch about the pointer), a press held on the fitted photo (100 % there), a click on the zoomed photo (fit); steps land on 100 % when they cross it (`editing.md` §3). The pan is the canvas's own: `canvas:edit` and the navigator show it |
| `edit.panelVisible` | bool | `edit.panelToggle {on?}` (Ctrl+9, View ▸ Show Edit Controls) |
| `edit.crop` | `null`, or the Crop draft `{ratio, rect}`: `ratio` `manual` \| `current` \| `"W:H"` (`"4:6"`, `"8.5:11"`), `rect` `[x, y, w, h]` normalised to the photo in the stored frame (`data-model.md` §3.3) or `null` before one is drawn | the Crop tool |
| `edit.straighten` | `null`, or `{angle}`, −10 … 10 degrees | the Straighten tool |
| `edit.retouch` | `null`, or `{patches: [{target: [x, y], source: [x, y], radius}], position, brush}`: the patches made (at most 1000, points normalised to the photo, radius 0.002 … 0.25 of its longer side), how many are in effect (0 … the number of patches; Undo Patch lowers it) and the brush radius (0.002 … 0.25) | the Retouch tool |
| `edit.tuning` | `null`, or a Tuning session's `{fill, highlights, shadows, temperature, neutral}`: 0 … 1, 0 … 0.5, 0 … 0.5, −1 … 1, and the neutral colour `[r, g, b]` (each 0 … 1) or `null` | the Tuning tab |
| `edit.effect` | `null`, or the open effect's parameter panel (M12; set exactly while `edit.tool` is `effect`, replaced whole): `{name, params, drag, strokes, brush, erase, picker, eyedropper}`: the effect as the recipe names it (a parameter effect of tabs 3–5, `soft_focus`; `effect.list` lists them), its parameter values by name (at most 16; a number in the range its catalogue row gives, a choice by name, a colour `"#rrggbb"`, a checkbox `true`/`false`; one left out is at its default), the crosshair drag under way (`null`, or `{part: "centre" \| "extent", from: [x, y]}`, the point normalised to the photo in the stored frame), Soften's strokes (at most 1000 `{points: [[x, y]…], radius, erase}`, at most 1000 points each, inside the photo, radius 0.005 … 0.5 of its shorter side), the brush radius (0.005 … 0.5, default 0.05) and Erase (`true`) or Paint, the colour parameter whose picker popup is open and the one the eyedropper is armed for (`null` for none). Nothing reaches the recipe before Apply | `edit.tool.open {tool: "effect", name}` (a parameter tile's click), the panel's controls, `edit.tool.action` (M12.16), `edit.tool.apply`, `edit.tool.cancel` (Esc), `edit.tool.reset` |

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

M10's field (starts fresh; not persisted): the Options dialog's draft, `null` while it is closed
(opening sets it, closing clears it). `ui.set` merges `options` field by field (a partial draft set
over `null` takes the defaults); its `shortcuts` is replaced whole. Texts are at most 4096
characters; nothing in the draft is applied before the dialog's Apply or OK (`design-system.md`
§11.3), and every option changes only through the engine's `options.set {values}`.

| Field | Values | Changed by |
|---|---|---|
| `options` | `null`, or `{tab, values, error, previewScale, previewFontSize, shortcuts}`: the tab shown (`general` \| `fileTypes` \| `slideshow` \| `viewer` \| `export` \| `shortcuts`, as in `tab:options.<tab>`; Slideshow and Photo Viewer since M11); the option values changed since the last Apply, by settings key (at most 256 keys of 1 … 128 characters; each value a null, a boolean, a number or a text); the last Apply's error (`null` after a successful one); the UI scale and font size previewed while the dialog is open (`"auto"` or 75 … 300 in steps of 25; `small` \| `medium` \| `large`; `null`: the saved ones), which revert when it closes without an Apply | `app.preferences {tab?}` (Tools ▸ Options…, Ctrl+,; Dalica ▸ Preferences… on macOS: General unless a tab is named), `app.shortcuts` (Help ▸ Keyboard Shortcuts…, Ctrl+/: Keyboard Shortcuts), the tabs; Apply (`button:dialog.apply`, enabled while the draft changes something) runs `options.set {values}` and the shortcut changes and keeps the dialog open, OK applies what changed and closes, Cancel and Esc drop the draft |
| `options.shortcuts` | the shortcut editor (`menus-and-shortcuts.md` §2.0): `{search, context, showUnbound, recording, conflict, changes, resetAll}`: the search text, the Context filter (`global`, `library`, `editView`, … as `app.setShortcut` names them; `null`: All), "Show commands without a shortcut", the key cell recording (`{command, context, slot}`, slot 0 or 1; `null`: none), a conflict waiting for [Replace] [Cancel] (`{cell, key, command, context}`: the cell, the key pressed, and the binding that already has it), the changed bindings (`[{command, context, slot, key}]`, `key` a key as the keymap writes it or `null` to remove it; at most 1024) and Reset All. Commands must be ones the keymap knows (`not_found`). While a cell records, every key press goes to the editor, not to the keymap or the dialog's Esc and Enter | the editor's controls (M10.12); Apply runs `app.setShortcut` per change and `app.resetShortcuts` for Reset All |

M11's fields (all start fresh; none is persisted). `ui.set` merges `import` field by field (its
`destination` is replaced whole); `slideshow` and `viewer` are replaced whole, a partial one taking
the defaults for the rest. Lists are at most 1 000 000 entries, texts 4096 characters (a path 4096
bytes); photos must exist (`not_found`).

| Field | Values | Changed by |
|---|---|---|
| `room`, `rooms` | the room in front and the rooms open in tab order, `library` first and each once (`library` \| `import` \| `collage`); the tab strip shows while a room besides the Library is open | `collage.open` (the Collage room's stub opener until the engine's `create.collage` opens it, M14.18; M14), `import.open` (File ▸ Import From…, Ctrl+M, the toolbar's Import: opens the Import room or brings it to the front), `room.select {room}` (a tab), `room.close {room?, confirm?}` (a tab's ×; the Library tab never closes; closing the Import room while an import copies asks "Stop importing?" first: `needs_confirm` with that prompt over `engine.execute` (§5.5) unless `confirm: true`, which cancels the import, what it copied staying; closing the Collage room with unsaved changes asks [Save Draft] [Discard] [Cancel], `dialog:collageClose`, unless `confirm: true`, which discards them; M14) |
| `import` | `{focus, selection, preview, destination}`: the Import room's focused item (the anchor of a Shift range), the items selected and the item in the preview, by the candidate ids `import.list` gives (`null` for none), and the destination strip's draft `{place, folderName, after}`: the folder imported into (a path; `null`: Options ▸ General's "Save imported pictures in"), the folder name (`null` until one is chosen; a text, `{"dateTaken": true}` or `{"today": true}`, as `import.run` takes it) and `leave` \| `delete` after copying. Kept while the app runs, so the room reopens as it was left | the Import room (M11.15, M11.20): `import.select {ids, mode?}` (a click: `replace`, Ctrl-click: `toggle`, Shift-click: `range` from the focused item; a group's clock replaces with the group), `import.selectAll` (Ctrl+A), the arrows `import.previous` / `next` / `up` / `down` and with Shift `import.extendPrevious` … `extendDown` (from the item in the preview; Up and Down by the tray's columns), X (`import.toggleExclude`) and Enter (`import.importSelected`) in the Import context; a new listing clears focus, selection and preview |
| `slideshow` | `null`, or `{photos, index, playing, bar, zoom, pan, transition, from}` while one plays (`view: "slideshow"` needs it, and it plays only there): the photos in order (1 or more), the one shown (0 … the last), playing or paused, the control bar shown, the zoom (1 fitted … 8), the point of the photo at the centre of the screen (`[x, y]`, each 0 … 1), the transition running (`{from, progress}`: the photo it leaves and 0 … 1; `null` for none) and where it started (`library` \| `edit` \| `viewer`, where Exit goes back to) | `view.slideshow` (View ▸ Slideshow, Ctrl+4, the edit top bar's Play; M11.19), and in the Slideshow context `slideshow.exit` (Esc), `slideshow.next` (→, PgDn), `slideshow.previous` (←, PgUp), `slideshow.first` (Home), `slideshow.last` (End), `slideshow.pause {on?}` (Space), `slideshow.zoomIn` (+), `slideshow.zoomOut` (−); `slideshow.transition {transition}`, `slideshow.captions {on?}` and `slideshow.displayTime {seconds}` keep the bar's settings in Options ▸ Slideshow (M11.21; with a library open, a show playing or not; the edit view's caption-bar toggle runs `slideshow.captions`) |
| `peek` | `null`, or the photo the quick full-screen preview shows while Ctrl+Alt is held | `peek {id}` (`null` ends it; M11.14 runs it from the held keys) |
| `edit.second`, `edit.focus`, `edit.layout`, `edit.showOriginal` | A│B's second photo on the right (or bottom) side, `edit.photo` being on the left (or top; `null` outside A│B; needs `edit.photo`; the photo the edit view works on, its tools, caption and filmstrip, is the focused side's); the side with the focus, `left` \| `right` (top and bottom when stacked); `sideBySide` \| `stacked`; bool: the focused side shown unedited | `edit.switchFocus` (⇄), `edit.switchLayout` (⇅), `edit.showOriginal {on?}` (`on` defaults to true: `\` pressed, Show original pressed; their release sends `false`), `edit.comparePhoto {id}` (the filmstrip in A│B: the focused side's photo) (M11.16) |
| `viewer` | `null`, or `{index, backdrop, controls, zoom}` while the photo viewer is open (`view: "viewer"` needs it): the photo shown by its place in `viewer.list` (its folder in name order), `translucent` \| `opaque`, the controls shown, the zoom (`"fit"` or 0.001 … 4, as `edit.zoom`) | `dalica --view FILE` (M11.12), and in the Photo viewer context `viewer.next` (→, K), `viewer.previous` (←, J), `viewer.zoomIn` (+, PgUp), `viewer.zoomOut` (−, PgDn), `viewer.actualSize` (1), `viewer.toggleStar` (8, Ctrl+8), `viewer.rotateRight` (Ctrl+R), `viewer.rotateLeft` (Ctrl+Shift+R), `viewer.trash` (Delete, asking first), `viewer.background` (Space), `viewer.openInDalica` (E), `viewer.close` (Esc); `viewer.settings` (Tools ▸ Configure Photo Viewer…) opens `dialog:viewerSettings`; `controls` set here shows or hides the controls until the next mouse move (M11.17) |

M13's fields (all start fresh; none is persisted): the people views, the People and Places panes
and the face model dialog's draft. `ui.set` merges `unnamed`, `people`, `peoplePane` and `places`
field by field (each of their fields replaced whole); `faceModel` is replaced whole, a partial one
taking the defaults for the rest. A face is written `"<PhotoId>.<n>"`, its photo and its place among
the photo's faces (0 … 255; `"12.0"`, as in `face:12.0`); a point `[latitude, longitude]` in
degrees. Lists of faces and photos are at most 1 000 000, of people 1 024, texts 4096 characters;
repeated entries are kept once; photos must exist (`not_found`), people are checked by the shape of
their id (`p` and 4 to 16 lowercase hex digits). Until the tasks that draw them land, the panes and
the grid are stubs that show what arrives; their commands that change the library check their
parameters, then are `disabled` with what they wait for ("Arrives with the Unnamed faces.").

| Field | Values | Changed by |
|---|---|---|
| `unnamed` | `{open, expanded, showIgnored, selection, names}`: the Unnamed grid (`organizing.md` §6.2) fills the light table's region while `open` (Unnamed selected in the People group); the header's [Expand Groups] and [Show Ignored Faces]; the faces selected; the name typed in each group's "Add a name" field by the group's number (`{"0": "Ana"}`, groups numbered from 0 largest first; at most 1 024 groups) | `faces.expandGroups {on?}`, `faces.showIgnored {on?}` (toggle without `on`; `button:faces.expandGroups`, `button:faces.showIgnored`); naming a group runs `faces.nameGroup {group, name?}`, its × `faces.ignoreGroup {group}`, Delete or the face menu's Ignore Face `faces.ignoreSelected {faces?}` (M13.21) |
| `people` | `{sort, zoomToFaces, onlySuggestions}`: the People group's order (`name` \| `count` \| `top10`, the view-options popup's Sort People by …); the people whose section shows their faces (Zoom to Faces, against Show Whole Photos); those whose section shows only its suggestions ("Show only suggestions") | `view.peopleSort {by}` (M13.25), `people.zoomToFaces {person, on?}`, `suggestions.only {person, on?}` |
| `peoplePane` | `{selected, hovered, manual}`: the people selected in the People pane (Ctrl-click; several filter the light table to the photos holding all of them); the face hovered in the pane (its box highlighted on the photo, and back; `null` for none); Add a Person Manually's box, the edit view only (`null`, or `{rect: [x0, y0, x1, y1], name}`: normalised to the photo as shown, x0 < x1, y0 < y1, and the name typed; nothing is stored before the name, through `faces.add`) | `people.filter {people}` (`[]` clears), `faces.addManually {on?}` (`disabled` outside the edit view; M13.24) |
| `places` | `{centre, zoom, pin, bubble, search, results, drop}`: the Places pane's map (`organizing.md` §7): its centre (latitude within ±85.05°, Web Mercator's reach) and zoom (0 … 19); the pin tool's pin (`null`, or `{at, place}`: the point and the place a search found there, `null` for one placed by hand); the bubble open (`null`, `"drop"`, `"pin"` or `{"marker": n}`, `marker:<n>`; `drop` needs `drop`, `pin` needs `pin`); "Search for a place:"'s text and its results (`[{name, at}]`, at most 50); photos dropped on the map waiting for the drop bubble's OK (`null`, or `{ids, at}`) | `map.zoomIn`, `map.zoomOut`, `map.pan {centre, zoom?}`, `places.pin {at?}` (the map's centre without `at`; `null` takes it away), `places.drop {ids, at}` (a drop of photos on the map), `places.bubble {marker? \| pin?}` (`{}` closes it and drops a drop), `places.put` (the bubble's OK), `places.showPhotos {marker?}`, `places.removeLocation {marker?}`, `map.search {text}` (typed coordinates place the pin; a name fills `results`), `map.showResult {n}` (the pin at result n, named after it, the map moved there, the list closed), `map.allowTiles {on}` (the pane's question: Allow or Not Now, remembered as `network.tilesAsked`) (M13.22, M13.28) |
| `faceModel` | `null`, or the face model dialog's draft (`faces.md` §3.1) while it is open: `{entry, file, accepted, progress}`: the model of `faces.models.list` offered or the local file being added (at most one of them; an id of 1 … 128 characters, a path of at most 4096 bytes), "I have read these terms and accept them for my own use" (Download is enabled only once it is ticked), and the download's progress (0 … 1, `null` while none runs) | `faceModel.open {entry?, file?}` (Options ▸ People's Download…, Find faces in my photos turned on without a model, the first open of People; M13.27), the dialog's controls; closing it clears it |

The person menu's rows (Rename…, Merge Into ▸ a person, Delete Person) and Tools ▸ Geotag ▸ Clear
Location… open their dialogs (`renamePerson`, `mergePeople`, `deletePerson`, `clearLocation`) whose
default button runs the engine's `people.rename`, `people.merge`, `people.delete` and `geo.clear`
(openers, as Rename… below); Copy Location and Paste Location run the engine's `geo.copy {id}` (the
focused photo) and `geo.paste {ids}` (the selection). The UI's own commands never take an engine
command's id (`faces.ignore`, `faces.confirm`, `map.allow` stay the engine's), so `engine.execute`
always reaches the engine: the face menu runs `faces.moveToNewPerson`, `faces.moveToPerson {person}`,
`faces.removeFromPerson`, `people.thumbnail {face?}` and `faces.ignoreSelected`, and the suggestion
strip `suggestions.confirm {faces?, person?}`, `suggestions.reject {faces?, person?}`,
`suggestions.confirmAll {person}` and `suggestions.findMore {person}` (M13.30; without `faces`, the
faces selected: the Unnamed grid's, or in a person's section theirs on the photos selected there;
with `person`, Confirm names Find More's faces as that person through `faces.move`).

M14's fields (they start fresh; none is persisted): the Collage room's view state. `ui.set` merges
`collage` field by field (each of its fields replaced whole). Everything in it but `tab` is empty
while the Collage room is not open (`bad_params` otherwise), and closing the room empties it. A
picture on the canvas is named by its number among the engine's placements (0 … 999, as in
`item:<n>`); lists hold at most 1 000 entries, repeated ones kept once; clips must be photos that
exist (`not_found`). The collage itself (its style, settings, clips and placements) is the
engine's (`collage.inspect`, M14.16), never view state. Until the tasks that draw them land
(M14.18, M14.19), the room and its canvas are stubs; their commands that change the collage check
their parameters, then are `disabled` with what they wait for ("Arrives with the Collage room.",
"Arrives with the collage canvas.").

| Field | Values | Changed by |
|---|---|---|
| `collage` | `{tab, selection, clips, gesture, getMore, close}`: the left panel's tab (`settings` \| `clips`, kept while the room is closed); the pictures selected on the canvas (`[n]`, outlined in `accent`); the clips selected in the Clips tab (`[PhotoId]`); the drag or turn under way (`null`, or `{item, kind, from, to}`: the picture, `move` \| `turn` (the wheel handle), and where the pointer went down and is now, `[x, y]` normalised to the page, each −1 … 2); Get More (the Library tab in front while the tray offers Back to Collage; never with the room in front); the close question while it asks (`null`, `tab` or `window`: what closes) | `collage.open {ids?, section?}`, `collage.tab {tab}` (`tab:collage/<tab>`), `collage.selectClips {ids}`, `collage.getMore`, `collage.backToCollage` (`button:tray.backToCollage`), `collage.closeRoom {discard?}` (Close; as `room.close {room: "collage"}`), and on the canvas `collageCanvas.select {items, add?}` (a click, Ctrl and Shift add), `collageCanvas.selectAll` (Ctrl+A, M14.19), `collageCanvas.selectNone` (Esc) in the Collage context; `room.select {room: "collage"}` ends Get More (M14.18, M14.19) |

The room's other commands are the UI's own, never the engine's ids (`create.collage` and the
`collage.*` of the engine's collage session stay the engine's, so `engine.execute` always reaches
them): `collage.style {style}` (`pile` \| `mosaic` \| `frameMosaic` \| `grid` \| `contactSheet` \|
`multipleExposure`), `collage.settings {borders?, spacing?, background?, usePicture?, format?,
orientation?, shadows?, captions?}`, `collage.addClips` (+), `collage.removeClips` (−),
`collage.createCollage`, `collage.saveAsDraft` and `collage.resetSettings` (M14.18), and on the
canvas `collageCanvas.move {item, centre}`, `collageCanvas.turn {item, angle, scale}`,
`collageCanvas.order {item?, to}` (Bring to Top / Send to Bottom), `collageCanvas.swap {a, b}`,
`collageCanvas.remove {items?}` (Remove, Delete in the Collage context), `collageCanvas.setBackground
{item?}`, `collageCanvas.setFrameCenter {item?}`, `collageCanvas.scramble`, `collageCanvas.shuffle`
and `collageCanvas.viewAndEdit {item?}` (M14.19). Ctrl+Z and Ctrl+Y in the Collage context are the
engine's `collage.undo` and `collage.redo`, the room's own history (M14.16). Create ▸ Picture
Collage… is greyed until the engine registers `create.collage`.

M15's fields (they start fresh; none is persisted): the player while the edit view shows a video,
the Open With ▸ submenu, a drag out of the window, the default-viewer question's draft and the
language in effect. `ui.set` merges `video` field by field (each of its fields replaced whole); the
others are replaced whole. Lists hold at most 1 000 000 photos (handlers 64, file types 64),
repeated entries kept once; photos must exist (`not_found`). The video, its bar and its tab are
the edit view's player (M15.21, M15.26); until the tasks that draw them land (M15.22, M15.24), the
Open With ▸ rows and the question are stubs whose commands check their parameters, then are
`disabled` with what they wait for ("Arrives with OS integration.").

| Field | Values | Changed by |
|---|---|---|
| `video` | `{playing, position, trim, volume, fullScreen}`: playing or paused; the playhead in milliseconds from the clip's start (0 … 86 400 000); the trim ruler's handle being dragged (`null`, `start` \| `end`); the volume (0 muted … 100; 80 to start, carried from one video to the next); the video filling the screen. Everything but `volume` only while the edit view shows a video (its photo's facts say so; `bad_params` otherwise), and idle again once it no longer does. Playing, the playhead and the volume follow the engine's player each frame (M15.26): a change `ui.set` makes is sent to it, and one it refuses (a clip Dalica cannot play) does not stay | `player.play`, `player.pause`, `player.toggle` (`/` and Space while a video is shown, `button:video.toggle`), `player.seek {ms}` (the scrubber), `player.step {frames}`, `player.volume {volume}`, `player.fullScreen {on?}` (`button:video.fullScreen`); `trim` while a trim ruler's handle is dragged |
| `openWith` | `null`, or `{photo, handlers}` while the Open With ▸ submenu is open: its photo and the applications the system offers for it (`null` while they are asked: "Loading…"; `[]`: "No other applications"; each `{id, name}`, the id as the system names it, 1 … 512 bytes, the name 1 … 256 characters) | `openWith.list {id?}` (the submenu opening: the focused photo without `id`; `disabled` until the engine has `photo.openWithList`, M15.17), `openWith.close` |
| `dragOut` | `null`, or `{ids}`: a drag of photos that has left the window, the photos whose files the system's drag carries (a copy; M15.23) | the drag (M15.23) |
| `defaultViewer` | `null`, or `{types}` while the default-viewer question is open (`dialog:defaultViewer`): the file types chosen for Dalica by the names `viewer.fileTypes` gives (`jpeg`, `png`; 1 … 16 lowercase letters and digits), none to start with | `defaultViewer.ask` (offered once after the first run, M15.24), its checkboxes; [Use Dalica] (enabled once a type is chosen) runs the engine's `viewer.setDefault {types}`, [Not Now] closes it |
| `language` | the language the interface is drawn in, as a tag (`en`, `pt-BR`, `qps` the pseudo-locale; never `system`, which is resolved first): the setting `interface.language` mirrored for the next draw | M15.7 |

The player's other commands: `player.step {frames}` (`,` and `.`, −1 and 1, while a video is
shown; held, rewind and fast-forward), `player.setStart {ms?}` and `player.setEnd {ms?}` (the trim
at `ms`, else at the playhead; one step of the video's history), `player.saveFrame` (into Video
Frames, kept to the write roots under automation, §5.7) and `player.exportClip {path?}` (the save
picker without `path`). Play, the steps, seeking and Save Frame are refused with the reason on a
clip Dalica cannot play; the trim and Export Clip work on any clip. Each drives the engine's
player; the ids are the UI's own: the engine's `video.*` (M15.25) stay the engine's, so
`engine.execute` always reaches them. In the library and
on a still photo the video keys are what they were (Space stars the photo in the edit view).

Locate on Disk, Open With, Open in External Editor, Copy Path and the system clipboard are the
engine's commands (`photo.locate {id}`, `folder.locate {id}`, `photo.openWithList {id}`,
`photo.openWith {id, handler}`, `photo.openExternal {ids}`, `photo.copyToSystem {ids, picture?}`,
`photo.copyPath {ids}`; M15.17), which their menu rows and keys run with the focused photo (the
open one in the edit view), the selection or the current folder; until the engine has them the
rows are greyed "Arrives with OS integration.". The UI's own are the export notice's
`toast.showFolder {path}`, the viewer's `viewer.showInFileManager` and `defaultViewer.ask`.

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

Everything in `ui.state`, plus (people's names withheld without `--automation-allow-people`, §6.4):

| Field | Content |
|---|---|
| `window` | `{width, height, pixelsPerPoint, focused, drawing}` — `drawing` false while the compositor delivers no frames |
| `regions` | `[x, y, w, h]` per region of `ui-layout.md` §2: `menuBar`, `toolbar`, `tabStrip`, `tree`, `splitter`, `banner` (the filter banner, in search mode only: the 32 rows over the light table and its scroll bar, which start under it), `lightTable`, `scrollbar`, `sidePane`, `statusLine`, `tray`, `trayButtons`, `bottomControls`, `actionButtons`; while a room besides the Library is open `tabStrip` (28 high under the menu bar, the rest moved down by it), and with a room in front only `menuBar`, `tabStrip`, `room` (the room's content) and `statusLine` (at the bottom of the window); a slideshow and the photo viewer register none; in the edit view (`editing.md` §2) `menuBar`, `editTopBar`, `filmstrip` (inside the top bar), `editPanel` (`null` while hidden), `canvas` (the photo area above the caption bar), `captionBar`, `sidePane` and the bottom area's four, while the library's own are `null`; M13: `people` and `places`, the People and Places panes' bodies, once those panes are drawn (M13.24, M13.22); M14: `collage`, the Collage room's content (inside `room`) while it is in front; M15: `video`, the video over `canvas` while the edit view shows one; `null` when not shown |
| `scroll`, `scrollViews` | `scroll` is `ui.state`'s anchor (`{section, offset}`); `scrollViews` is `{lightTable, tree, topSection, sticky}` — the light table's and the tree's offsets in points as last drawn, the section at the top, the pinned header (section ids as in `ui.state`, `null` for none) |
| `hovered` / `focused` | widget ids |
| `popup` | the open menu, context menu or popup as last drawn, `null` when none: `{kind, menu?, items: [{widget, id, label, enabled, checked}]}`; `kind` is `menu` (with `menu`, the bar menu's key: `File`), `viewOptions` (the toolbar's popup) or `contextMenu`; `id` is the row's command (`null` for a submenu row) |
| `dialog` | the top dialog, `null` when none: `{kind, widget, title, fields: {name: value}, buttons: [widget ids], default, command?}` (`default` the widget of the button Enter presses). The dialogs whose draft is a field of the view state (M8's, M9's, M10's Options: `options`) have empty `fields` |
| `filePicker` | the stubbed picker waiting for `ui.filePicker` (§5.6): `{command, kind, title}`, and for the save picker `name`, `folder` and `params` |
| `status`, `toasts` | the status-line text as drawn; the toasts showing, oldest first: `[{id, text, kind, remaining}]` (`id` the toast's widget `toast:<n>`, `kind` `info` or `error`, `remaining` in seconds) |
| `tooltip` | the text of the tooltip the last frame showed (its lines joined with `\n`), `null` when none |
| `drag` | a drag of photos in progress, `null` when none: `{from, ids, target, op, pill}`: the widget it started on (`thumb:12`, `thumb:12/in/a91c0e`, `tray:item:12`, `tray:token`), the photos it carries (the selection when the photo was selected, else that photo; from the tray the tray-selected photos when the mini is tray-selected, else that one), the drop target under the pointer (a widget id: `tree:folder:<id>`, `tree:album:<id>`, `tray`, or for the gap beside a cell of a user album the cell's `thumb:<id>/in/<album>`; `null` for none, and where a drop would change nothing: the photos' own folder, an album or a tray already holding them all, the gap beside a dragged photo), what a drop there would do (`move`, `copy`, `add`, `reorder`, `hold`, `null`) and the drag pill's text ("Move 3 photos to Garden", "Copy 1 photo to Garden", "Add 3 photos to Lisbon", "Hold 3 photos in the tray", "Reorder 3 photos", "3 photos" over nothing); it ends when the button is released (dropping there) or Esc is pressed (dropping nothing). M13: a drag of faces (from `face:<id>`) carries `faces` (`["12.0", …]`) and no photos, a person's row `tree:person:<id>` takes them (`name`: "Name 2 faces", the person not named in the pill yet; "2 faces" over nothing), and photos over the Places pane's map `map:places` offer `geotag` (the drop opens the bubble "Put %d photos here?"). Not part of `ui.state` |
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
(`ui.clickWidget` on the tree row, or `ui.scroll`). Ids are in §7. Without
`--automation-allow-people` a label that shows a person's name reads the placeholder Person, the
rest of it kept, under the same id (§6.4).

### 6.8 Clicks, drags, hover and `ui.pointer`

- `ui.clickWidget {id}` clicks the widget's centre; `fx`, `fy` (0..1) pick another point inside it;
  `count: 2` double-clicks; `button: "right"` opens context menus; modifier flags apply to the click
  (`ctrl: true` toggles a thumbnail in the selection, `shift: true` extends it). A widget under an
  open dialog (drawn before its backdrop, which takes every click meant for the window) is
  `disabled`, naming the dialog by its id (`dialog:about`), and nothing is played (M14.25); the
  dialog's own widgets and popups are clicked as usual.
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
  Ctrl+Alt full-screen preview, `\` before/after (M11). A key left down is released when the connection closes.
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

`ui.menu.tree {context: "photo" | "section" | "tray" | "treeGroup" | "tag" | "face" | "person"}` returns that
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
`tray`, `photo`, `collage` (M14: a picture on the collage canvas: Bring to Top · Send to Bottom · View and
Edit, greyed until M14.19), `person` (M13.25: Rename…, Merge Into ▸ the other people by name, Delete Person, for the person
whose section is selected; greyed with none) and `face` (M13.21: Move to New Person… · Move to Person ▸ the people by name, or a greyed
"No people yet" · Remove from Person · Set as Person Thumbnail · Ignore Face, for the faces
selected) answer now, the others are `unsupported` until then. Without
`--automation-allow-people` the rows naming a person read the placeholder Person (their `widget`
and `params` keep the person's id), and `ui.menu.invoke {path}` matches such a row by the
placeholder only (§6.4).

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
- `headless: true` draws the UI on the CPU (§8) instead of reading back the window, the same
  picture `--headless` paints, byte for byte: it needs no compositor and works when the window is
  occluded, minimised or the display is asleep. The window keeps a CPU copy of every texture it
  uploads while the control channel is on (`--control`); the capture settles over the window's
  frames, then paints the next one from that copy. A window that draws no frames gets no passes
  from the system, so the capture runs its own, one per logic tick, and hands what they uploaded
  to the window's next frame. It draws at the window's scale (`scale` must be left out or match:
  else `bad_params`). On Wayland a minimised window gets no logic ticks either, so the 70 s
  request limit answers.
- Without `headless` the window reads its own frame back from the GPU; a window that draws no
  frames (minimised, covered) then answers `timeout` within **2 s** (as every input request does).
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
`batchEdit`, `metadataWrite`, `rebuild`, `duplicates`, `faces`, `network` (a face model download
or map tiles, only after the user allowed it, M13), `collage`, `analysis`. `jobs.cancel` stops a job at its next photo boundary: photos already written stay
(each was written atomically), nothing is left half-written. Cancelling a job is never an undo of
the photos it completed.

An export of a person's section (`export.run` with `section: "person:<id>"`: the Export dialog's,
whether a click or a control request confirmed it, or a client's own) makes its folder under the
name suggested from theirs. The session marks the job, and when it ends the folder it made
(M14.25, M15.42; `automation::PersonExports`); without `--automation-allow-people`:

- that job's entry, in `jobs.list`, `jobs.wait` and `ui.inspect`'s `jobs`, has `result.folder`,
  `result.name` and `result.written` `null`, its counts kept (`automation::withhold_export_folder`,
  as the draft's `export.folderName` is withheld, §6.4);
- every path into that folder is `null` wherever a job or a toast gives it: any job's entry (the
  scan that brings the folder into the library: `result.roots`, `result.walked`) and the toast's
  `action.folder` in `ui.inspect`'s `toasts` (`automation::withhold_export_paths`);
- that job's failed and skipped photos keep their `file` but have their `reason` `null` (a reason
  may name the path written to), and a failed job's `error` is `null`
  (`automation::withhold_export_folder`);
- `ui.widgets` and `ui.inspect` give the folder's tree row (`tree:folder:<id>`, its count kept) and
  its section's header, icon and title (`section:folder:<id>`, `/icon`, `/title`) the placeholder
  `Person`;
- in the edit view on a photo of that folder, the status line's folder prefix reads `Person` in
  `ui.state`'s and `ui.inspect`'s `status` and in `text:status` (`Person › IMG_0001.jpg …`; the
  window draws the folder's name);
- the toast of that job when photos failed (`ui.inspect`'s `toasts`, `toast:<n>` in `ui.widgets`)
  leaves out the reasons: "Exported 0 pictures; 2 failed. Could not export IMG_0001.jpg, and 1
  more" (the window shows them);
- searches with `withholdNames` (§6.4) find no photo through the folder's name, and
  `search.suggest` (sent with `withholdNames` too) suggests no such folder.

Headless MCP gives its jobs the same way (`mcp.md` §5: `jobs.list`, `jobs.wait` and the final
entry a tool that started a job answers).

The job and the folder are known by the job and the path the export made, never by matching a
folder's name against people's names. The marks last for the session (M14.25, M15.42, M15.51).

## 7. Widget ids

Every interactive widget registers an automation id with its rect each frame (`architecture.md`
§7). Ids are stable across releases and never translated (`localization.md`); renaming one is a
breaking change for scripts and tests.

Grammar: `kind ":" name ("/" part)*` — `kind` is lowercase; `name` is a command-like dotted name or
an id; `part` names a sub-widget of a composite (section headers). Effect and slider names are the
recipe operation and parameter names of `data-model.md` §3.1 and `editing.md` (snake_case).

| Region (milestone) | Ids |
|---|---|
| Regions (M3) | `region:<name>` for each region of §6.6 shown (`region:menuBar`, `region:toolbar`, `region:tree`, `region:splitter`, `region:banner` (M9, search mode), `region:lightTable`, `region:scrollbar`, `region:sidePane`, `region:statusLine`, `region:tray`, `region:trayButtons`, `region:bottomControls`, `region:actionButtons`; M11: `region:tabStrip`, `region:room`): the region's rect, no input of its own (role `other`); a region not shown is not listed |
| Menu bar (M3) | `menu:File` … `menu:Help` (title by its English label; the fourth is `menu:Folder` whatever its shown title; macOS adds `menu:Dalica`), `menuitem:<command id>` for open menu and context-menu items (`menuitem:<command id>/<variant>` for rows of one command that differ by a parameter, `menus-and-shortcuts.md` §1; M9: the Add to Album rows of the photo menu and of the tray's Add to Album ▾, one per user album, are `menuitem:album.add/<AlbumId>`), `submenu:<menu>/<label>…` (`submenu:View/Thumbnail Caption`), and on a menu taller than the window its scroll arrows `button:menu.scrollUp/<depth>` and `button:menu.scrollDown/<depth>` (depth 1 for a menu of the bar or a context menu) |
| Title bar (M3) | Windows and Linux, unless the system title bar is chosen (`design-system.md` §11.1): `button:window.minimize`, `button:window.maximize` (labelled Restore while maximized), `button:window.close`, `titlebar:drag` (the free gap: drag moves, double-click maximizes; also on macOS, beside the traffic lights); in a real window, while neither maximized nor full screen, the resize edges `resize:n`, `resize:s`, `resize:e`, `resize:w`, `resize:ne`, `resize:nw`, `resize:se`, `resize:sw` |
| Toolbar (M3) | `button:toolbar.import`, `button:toolbar.newAlbum`, `button:tree.flat`, `button:tree.tree`, `button:toolbar.viewOptions`, `button:filter.starred`, `button:filter.faces`, `button:filter.videos`, `button:filter.geotagged`, `slider:age`, `field:search`, `button:search.clear`, `indicator:activity`; M9: the search field's suggestions while listed, `field:search/option/<n>` (role `suggestion`, from 0 at the top, active while highlighted) |
| Room tabs (M11+) | `tab:room/library`, `tab:room/import`, `tab:room/collage` (M14), active for the room in front; `tab:room/<room>/close` on every tab but the Library's; `tab:room/more` when tabs overflow |
| Collage room (M14) | the room's content `region:collage`; its left panel's tabs `tab:collage/settings` and `tab:collage/clips` (role `tab`, active while shown); the canvas `canvas:collage` (the page at its ratio) with a picture `item:<n>` (its number among the engine's placements, bottom first, active while selected; M14.19), the wheel handle `canvas:collage/handle` on the one picture selected in a pile (a drag turns and scales it round its centre), while the background's eyedropper is armed the page's catch `canvas:collage/eyedropper`, the buttons above the page `button:collageCanvas.selectAll`, `button:collageCanvas.selectNone`, `button:collageCanvas.remove` and `button:collageCanvas.setBackground` (`button:collageCanvas.setFrameCenter` in its place in a Frame Mosaic) and below it `button:collageCanvas.scramble`, `button:collageCanvas.shuffle` and `button:collageCanvas.viewAndEdit`; the Settings tab (M14.18): the style `choice:collage.style` (its rows `choice:collage.style/<style>`), Picture borders `toggle:collage.border.none`, `toggle:collage.border.white`, `toggle:collage.border.instant`, Grid spacing `slider:collage.spacing`, Background `choice:collage.background/solid` with its swatch `color:collage.background` (its popup's parts as the effects' `color:` swatch) or `choice:collage.background/picture`, Page format `choice:collage.format` (its rows `choice:collage.format/<n>`) with `button:collage.deleteFormat` (custom ratios only) and `toggle:collage.orientation.landscape`, `toggle:collage.orientation.portrait`, `check:collage.shadows`, `check:collage.captions` (each disabled for a style that does not offer it), and along the bottom `button:collage.create` (while a collage is written `progress:collage` and `button:collage.cancel`), `button:collage.reset`, `button:collage.close`; the Clips tab (M14.18): a clip `clip:<PhotoId>` (active while selected), `button:collage.addClips`, `button:collage.removeClips`, `button:collage.getMore`; in the library while Get More is on the tray's `button:tray.backToCollage`; the picture menu's rows `menuitem:collageCanvas.order/top`, `menuitem:collageCanvas.order/bottom`, `menuitem:collageCanvas.viewAndEdit`; in the edit view `button:edit.editCollage` for a collage the engine can reopen (M14.20); the close question `dialog:collageClose` with `button:dialog.saveDraft` (the default), `button:dialog.discard` and `button:dialog.cancel` |
| Video in the edit view (M15) | while the edit view shows a video: `region:video` (over the photo area, `region:canvas`), its bar `button:video.toggle` (Play / Pause, active while playing), `slider:video.position` (the scrubber), `handle:video.trimStart` and `handle:video.trimEnd` (the trim ruler's handles), `button:video.setStart`, `button:video.setEnd`, `slider:video.volume`, `button:video.fullScreen` (active while full screen); the video tab's `button:video.saveFrame` and `button:video.exportClip`; the scrubber's readout `slider:video.position/readout` and the volume's speaker `slider:video.volume/mute` (M15.26); Play / Pause and Save Frame disabled with the reason on a clip Dalica cannot play, the volume on one whose sound it cannot decode. The OS integration: `menu:photo.openWith` (the Open With ▸ submenu, M15.22), `dialog:defaultViewer` with `button:dialog.use` and `button:dialog.cancel` (Not Now) |
| Import room (M11.15) | `choice:import.source` ("Import from:", labelled with the source listed or "Choose a source") and, while its menu is open, `choice:import.source/<n>` (the cards, drives and recent sources in `import.sources`' order, from 0) and `choice:import.source/folder` (Folder…); with no source listed `empty:import` and its `button:import.chooseFolder`; the tray's `text:import.status` (labelled with the status line shown: "6 files · 5 to import"), `check:import.excludeDuplicates`, `importTray` (its empty space), `group:import/<n>` (a group header, from 0 in capture order, labelled with its period) with its clock `group:import/<n>/select` (active while the whole group is selected), `candidate:<id>` (a cell, by `import.list`'s id, labelled with the file name, active while selected) with `candidate:<id>/excluded` (the ⊘ of a picture left out: "Excluded" or "Already in your library") and `candidate:<id>/star`, `scroll:import` and `scroll:import/thumb` while the cells do not fit; the context menu's `menuitem:import.exclude`, `menuitem:import.exclude/include`, `menuitem:import.excludeAll`, `menuitem:import.excludeAll/include`; the preview's `text:import.preview` (labelled "filename · date time · size"), `image:import.preview`, `button:import.star`, `button:import.exclude` (active while excluded), `button:import.previous`, `button:import.next`, `button:import.rotateLeft`, `button:import.rotateRight`; M11.20: the destination strip `importDestination` with "Import to:" `choice:import.destination` (labelled with the folder's name; items `/default`, `/<n>` for the recent destinations, `/choose`), "Folder name:" `field:import.folderName` and its choices `choice:import.folderName` (items `/dateTaken`, `/today`, `/<n>` for the recent names), "After copying:" `choice:import.after` (cards and drives only; items `/leave`, `/delete`), the hint `text:import.hint` (labelled with its text), `button:import.importAll`, `button:import.importSelected` (labelled "Import Selected (n)"), `button:import.cancel`; while an import copies `progress:import` (labelled "Copying 45 of 340 at 38 MB/s"); after one cancelled or with failures `text:import.result` and `text:import.failure/<n>` (from 0; the last may be "and n more") |
| Slideshow (M11.19, M11.21) | `canvas:slideshow` (the whole window: a mouse move shows the bar, the wheel steps, a click goes on while the bar is hidden, a drag pans while zoomed; labelled "Slideshow: photo <id>"), `text:slideshow.caption` (the caption plate, labelled with the caption, while captions are on); while the control bar shows (any opacity above 0) `controls:slideshow` with `button:slideshow.exit`, `button:slideshow.zoom` (the magnifier, active while zoomed), `slider:slideshow.zoom` (1 fitted … 8), `button:slideshow.rotateLeft`, `button:slideshow.rotateRight`, `button:slideshow.previous` (disabled on the first photo without Loop), `button:slideshow.play` (labelled Pause while playing), `button:slideshow.next`, `button:slideshow.star` (active while starred), `button:slideshow.transition` (its drop-up's rows `menuitem:slideshow.transition/<cut\|panAndZoom\|dissolve\|wipe\|circle\|push\|rectangle>`, the current one active), `check:slideshow.captions` (role `toggle`, active while captions are on) and the display time's `stepper:slideshow.displayTime` (labelled "3 seconds") with `stepper:slideshow.displayTime/minus` and `/plus` (disabled at 1 and 60); Options ▸ Slideshow: `check:option.slideshow.loop`, `check:option.slideshow.captions`, `slider:option.slideshow.displayTime` and its field `slider:option.slideshow.displayTime/value`, `choice:option.slideshow.transition` with its items `choice:option.slideshow.transition/<id>`, and, disabled until Dalica plays sound, `check:option.slideshow.music`, `field:option.slideshow.musicFolder`, `button:option.slideshow.musicFolder/browse`, `check:option.slideshow.videoSound` |
| People and Places (M13) | the Unnamed grid (in the light table's region while `unnamed.open`): `button:faces.expandGroups` and `button:faces.showIgnored` (active while on), a group `group:<n>` (from 0, largest first; labelled "4 faces", or "4 of 9 faces" while it shows its first row only; active while one of its faces is selected) with its × `group:<n>/ignore` and its "Add a name" field `field:faces.name/<n>` (the people offered as it is typed: `field:faces.name/<n>/option/<k>`, role `suggestion`), with Show Ignored Faces the ignored faces' card `group:ignored`, the empty space `lightTable` (rubber band; as the light table's), a face `face:<PhotoId>.<n>` (its photo and its place among the photo's faces; also in a person section's Zoom to Faces cells and the People pane; role `thumbnail`, active while selected); suggestions (M13.30): a suggested face's ✓ `face:<PhotoId>.<n>/confirm` and × `face:<PhotoId>.<n>/reject` (role `button`; under its tile in the Unnamed grid, beside "Ana?" in the People pane's "Suggestions:"), in a person's section the suggested face's tile `face:<PhotoId>.<n>/in/<PersonId>` (labelled "Ana?") with its own `/confirm` and `/reject`, and the strip in the header's description row: `section:person:<PersonId>/confirmAll`, `/confirm`, `/reject`, `/findMore` (role `button`) and the checkbox `/onlySuggestions` (role `toggle`, active while on); the People pane's body `region:people` (M13.24: a person's row `person:<PersonId>`, active while selected; an unnamed face's "Add a name" `field:people.name/<PhotoId>.<n>`; in the edit view `button:faces.addManually`, active while the box is drawn) and the Places pane's `region:places` (its body, as `pane:places`) with its status line `text:places.status` ("9 places in the selection"); the map `map:places` with its markers `marker:<n>` (a place's number among the selection's places; a group of markers by its first; labelled with the count), `button:map.zoomIn`, `button:map.zoomOut`, the pin tool `button:map.pin` (active while armed), the pin `map:places/pin` (also a drop's point while its bubble asks), the bubble `map:places/bubble` (labelled with its text) and its buttons `button:places.put` (OK), `button:places.cancel`, `button:places.showPhotos` and `button:places.removeLocation`, and "Search for a place:" `field:places.search`; the map's attribution line `text:places.attribution` (labelled with its text: "Natural Earth" while no tile shows, else the tile provider's), the one-time question `map:places/question` (labelled "Show map tiles from OpenStreetMap? …") with `button:map.allowTiles` (Allow, the default) and `button:map.notNow`, and the places a name search found: `list:places.results` (labelled "3 places found"), a row per place `field:places.search/result/<n>` (from 0, labelled with the place) and its close button `button:places.results.close` (M13.28); in the edit view, with the People pane open, a face's box on the photo `facebox:<PhotoId>.<n>` (labelled with its person's name, active while highlighted) and Add a Person Manually's box `facebox:manual` with its handles `facebox:manual/nw`, `/n`, `/ne`, `/e`, `/se`, `/s`, `/sw`, `/w`, its name field `field:faces.manual`, `button:faces.manual.add` and `button:faces.manual.cancel` (M13.24); the face and person menus' rows `menuitem:faces.moveToNewPerson`, `menuitem:faces.moveToPerson/<PersonId>` (Move to Person ▸ `submenu:Face/Move to Person`; with no people the greyed `menuitem:faces.moveToPerson/none`), `menuitem:faces.removeFromPerson`, `menuitem:people.thumbnail`, `menuitem:faces.ignoreSelected`, `menuitem:people.rename`, `menuitem:people.merge/<PersonId>` (Merge Into ▸ `submenu:Person/Merge Into`), `menuitem:people.delete`; Options' `tab:options.network` and `tab:options.people` (between Slideshow and Photo Viewer); on Options ▸ Network (M13.28) `check:option.network.allowMapTiles`, the fields `field:option.network.tileServer`, `field:option.network.tileAttribution`, `field:option.network.geocoder`, `field:option.network.proxyHost` and `field:option.network.proxyPort` (the last two enabled for Manual), the proxy's radios `choice:option.network.proxy/system` and `choice:option.network.proxy/manual`, and the disabled `choice:option.network.logLevel` |
| Previous organizer's data (M14.15) | the first-run notice's toast `toast:<n>` with its `button:toast.ok` (it runs `legacy.noticeShown`, §6.4; the toast stays until pressed); Folder Properties' Photo data row `label:dialog.source` (labelled with the source's sentence, `legacy.inspect`'s `source`); the edit panel's line `label:edit.legacyEdits` (a recipe with entries kept as `legacy:`, `photo.inspect`'s `legacyEdits`); Options ▸ General's `check:option.files.readLegacyData`. The engine commands behind them, `legacy.inspect` and `legacy.noticeShown`, are in §6.4 |
| Library tree (M4) | `tree:group:albums` · `people` · `projects` · `folders` · `other` · `hidden` (the group header), `tree:folder:<FolderId>`, `tree:album:<AlbumId>` (also `tree:album:starred`, `tree:album:recent`, `tree:album:search`), `tree:person:<PersonId>` and `tree:person:unnamed` (M13.25: the People group's rows, labelled "Ana (12)", "Unnamed (4)"; Unnamed active while its grid shows), `tree:disclosure:<FolderId>` (tree mode); in tree mode a directory between a watched root and its folders that is no library folder itself is `tree:dir:<hash>` (sixteen lowercase hex digits of the 64-bit FNV-1a hash of its path's UTF-8 text, stable for the path) and its triangle `tree:disclosure:dir:<hash>` (such directories fold for the session only: `collapsedFolders` holds folder ids); `splitter:tree` |
| Light table (M4) | `section:<section id>` (the header block, e.g. `section:folder:12`), its parts `section:folder:12/icon`, `/title`, `/description`, `/play`, `/collage`, `/selectStarred`, `/save`, `/export`, and with unsaved edits `/saveEdited` (the Save text button at the top right); a person's section (`section:person:<PersonId>`, M13.25) has `/play`, `/collage`, `/zoomToFaces` (active while it shows the person's faces) and `/thumbnail` (Set as Person Thumbnail) in place of the folder's buttons, and no `/description`; while a header is pinned, its pinned line carries `section:<id>`, `/icon` and `/title` (and `/saveEdited`), and the block under it keeps only the ids of the parts shown below that line; `thumb:<PhotoId>` and its star badge `thumb:<PhotoId>/star` (a photo's cell in its folder's section); in an album's section the same photo's cell is `thumb:<PhotoId>/in/<album>` (`<album>` an album id, `starred`, `recent` or `search`; M9; in a person's section their PersonId, `thumb:<PhotoId>/in/<PersonId>`, M13.25, whose Zoom to Faces cells are the face tiles `face:<PhotoId>.<n>`), its badge `thumb:<PhotoId>/in/<album>/star`, and a click there selects the photo in the album; `lightTable` (empty space: rubber band, section menu); with no watched folders the empty state `empty:lightTable` and its `button:empty.addFolder`, `button:empty.import`; `banner:search` and `button:search.showAll` (M9), and the empty state of a search with no match `empty:search` (M9) |
| Scrollbar (M4) | `scroll:prevSection`, `scroll:up`, `scroll:track`, `scroll:thumb`, `scroll:down`, `scroll:nextSection` |
| Status line (M4) | `text:status`, `indicator:unsaved`, `indicator:temporarySession` |
| Photo Tray (M3: the box and its buttons, disabled; M4: its contents) | `tray` (the box), `tray:token`, `tray:item:<PhotoId>` (its held badge `tray:item:<PhotoId>/held`), `tray:more` (the "+N" place), `button:tray.hold`, `button:tray.clear`, `button:tray.addToAlbum` |
| Bottom controls (M3) | `button:star`, `button:rotate.ccw`, `button:rotate.cw`, `button:loupe`, `slider:thumbSize`, `toggle:pane.people`, `toggle:pane.places`, `toggle:pane.tags`, `toggle:pane.properties`; edit view: `button:zoom.fit`, `button:zoom.1to1`, `slider:zoom` |
| Action buttons (M3) | `action:export`, `action:collage`, `action:slideshow`, `action:more`; below 1100 wide only `action:more` is on the row, and the rows of its open menu carry the actions' own ids (role `menuItem`) |
| Side pane (M6) | `pane:tags`, `pane:people`, `pane:places`, `pane:properties` (the pane body), `button:pane.widen`, `button:pane.close`; Tags: `field:tags.add`, `button:tags.add`, `tag:<tag text>` and its remove button `tag:<tag text>/remove` (shown on the hovered row), `quicktag:1` … `quicktag:10`, `button:quicktags.configure`; Properties: `button:properties.showAll` (shown while something is selected) |
| Inline editors and About (M6) | the caption editor under a cell `field:caption.inline`; a header's description editor `field:section.description`; the About dialog's tabs `tab:about.about`, `tab:about.contributors`, `tab:about.models`; on Contributors the name toggle `choice:dialog.names/<username\|displayName\|realName>`, the list toggle `choice:dialog.list/<grabBag\|table>`, the sort dropdown `choice:dialog.sort` with its rows `choice:dialog.sort/<sort>`, `button:dialog.reverse`, and the table's headers `button:dialog.sort/<sort>` (`<sort>`: `name`, `prs`, `commits`, `linesAdded`, `linesDeleted`, `linesDelta`, `binaryAdded`, `binaryDeleted`, `firstCommit`, `lastCommit`) |
| Edit view (M7) | regions `region:editTopBar`, `region:editPanel`, `region:canvas`, `region:filmstrip`, `region:captionBar`; `button:edit.back`, `button:edit.play`, `button:compare.one`, `button:compare.two`, `button:compare.same`, `canvas:edit` (the photo area: click, double-click, press and hold, drag to pan; while comparing the focused side), `canvas:edit.other` (while comparing, the side without the focus: a click gives it the focus), `label:compare.selected` (the focused side's "Selected" pill), `button:compare.switchFocus`, `button:compare.switchLayout` (while comparing), `text:zoom` (the "Zoomed to 200 %" read-out, its text as the label, while zoomed), `navigator:edit` (the navigator while the zoomed photo is larger than the area: a click or drag centres the view there), `film:<PhotoId>`, `button:film.previous`, `button:film.next`, `field:caption`, `button:caption.toggle`, `button:edit.trash`, `tab:edit.basic`, `tab:edit.tuning`, `tab:edit.effects`, `tab:edit.effects2`, `tab:edit.effects3`, `button:edit.undo`, `button:edit.redo`, `button:edit.showOriginal`, `box:histogram` with its histogram `histogram:edit` and camera columns `box:histogram/info` (left out on a panel too short for both it and an effect tab's grid), the effect tabs' tiles `effect:<name>` (role `tool`) |
| Edit tools (M7) | `tool:crop`, `tool:straighten`, `tool:autofix`, `tool:autocontrast`, `tool:autocolor`, `tool:retouch`; `slider:basic.fill_light`; tool panels: Straighten's floating bar `box:straighten` with its `slider:straighten.angle`, `choice:crop.ratio` (its rows `choice:crop.ratio/manual`, `/current`, `/<W:H>` such as `/4:3`, `/custom/<name>` and `/add`), `button:crop.removeRatio`, `button:crop.suggestion/1` … `/3`, `button:crop.rotate`, `button:crop.preview`, the rectangle on the photo `crop:edit`, Retouch's `slider:retouch.brush`, `button:retouch.undoPatch` and `button:retouch.redoPatch`, `button:panel.apply`, `button:panel.cancel`, `button:panel.reset` |
| Tuning (M7) | `slider:tuning.fill_light`, `slider:tuning.highlights`, `slider:tuning.shadows`, `slider:tuning.temperature`, `button:tuning.autoLighting` (the Auto lighting wand), `button:tuning.neutralPicker` (the eyedropper, active while armed), `button:tuning.autoColor` (the Auto color wand), `button:tuning.reset` (left out when the tab is too short for it) |
| Effects (M12) | `effect:<name>` per tile, the effect as the recipe names it (tab 3: `effect:sharpen`, `effect:sepia`, `effect:bw`, `effect:warmify`, `effect:grain`, `effect:tint`, `effect:saturation`, `effect:soft_focus`, `effect:glow`, `effect:filtered_bw`, `effect:focal_bw`, `effect:graduated_tint`; tabs 4 and 5 by the names `effect.list` gives them); effect panels: `slider:effect.<param>` (`slider:effect.a`, `slider:effect.fade`), `check:effect.<param>`, `choice:effect.<param>` (Pixelate's blend mode; its menu items `choice:effect.<param>/<word>`, `choice:effect.mode/soft_light`), `color:effect.<param>` (a swatch: a click opens its picker popup `color:effect.<param>/popup`, holding the saturation / value field `color:effect.<param>/field`, the hue strip `color:effect.<param>/hue`, the hex field `color:effect.<param>/hex` and the eyedropper `color:effect.<param>/eyedropper`, also Vignette's pick from the photo), `button:effect.paint` and `button:effect.erase` (Soften's brush mode) and `slider:soften.brush` (its brush size), `crosshair:effect` (the crosshair on the photo), `button:panel.apply`, `button:panel.cancel`, `button:panel.reset` |
| Dialogs (M3+) | `dialog:<kind>` (the dialog: `dialog:clearTray`, `dialog:folderManager`, `dialog:firstRun`; M6: `dialog:folderProperties`, `dialog:adjustDate`, `dialog:quickTags`, `dialog:renameTag`, `dialog:about`, `dialog:releaseNotes`; M7: `dialog:customRatio` with `field:dialog.width`, `field:dialog.height`, `field:dialog.name`; M8: `dialog:saveExplain` with `button:dialog.save`, `dialog:revert` with `button:dialog.revert` and `button:dialog.undoSave`, `dialog:export` with `button:dialog.export` and its form (M8.20): `field:dialog.location`, `button:dialog.browse`, `field:dialog.folderName`, `check:dialog.numbers`, `choice:dialog.size/<original\|resize>`, the size field `slider:dialog.size/value` (its text as typed) and the slider `slider:dialog.size`, the quality dropdown `choice:dialog.quality` with its items `choice:dialog.quality/<automatic\|normal\|maximum\|minimum\|custom>` and the custom slider `slider:dialog.quality`, `choice:dialog.videos/<firstFrame\|fullVideo>` (disabled until M15), `check:dialog.watermark`, `field:dialog.watermarkText`, and the export progress window `dialog:exportProgress`, which is no dialog of the stack and has no backdrop; M9: `dialog:albumProperties` and `dialog:newFolder` with `button:dialog.ok`, `dialog:renameFiles` with `field:dialog.name`, `check:dialog.addDate`, `check:dialog.addSize` and `button:dialog.rename`, `dialog:moveFiles` with `button:dialog.move` and `check:dialog.dontAsk`, `dialog:copyFiles` with `button:dialog.copy`, `dialog:collision` with `button:dialog.renameDuplicates` and `button:dialog.skipDuplicates`, `dialog:removeFromAlbum` with `button:dialog.remove`, `dialog:deleteAlbum` with `button:dialog.deleteAlbum`, `dialog:deletePhotos` and `dialog:deleteFolder` with `button:dialog.moveToTrash`; M10: `dialog:options` with its tabs `tab:options.general`, `tab:options.fileTypes`, `tab:options.export`, `tab:options.shortcuts` and `button:dialog.ok`, `button:dialog.apply` (M11: `tab:options.slideshow`, `tab:options.viewer` between File Types and Export); on the File Types and Export tabs (M10.11) an on/off option `check:option.<key>`, a dropdown `choice:option.<key>` with its items `choice:option.<key>/<value>`, a number's slider `slider:option.<key>` and its field `slider:option.<key>/value`, a folder's path `field:option.<key>` and its `button:option.<key>/browse`, a text `field:option.<key>`, the RAW row's list `button:option.fileTypes.raw/list` with its rows `button:option.fileTypes.raw/list/<tiff\|cr3\|raf\|crw\|mrw\|x3f>`, and the rescan line `text:option.fileTypes.rescan`; on the Keyboard Shortcuts tab (M10.12) the search field `field:shortcuts.search` with `button:shortcuts.search/clear`, the Context filter `choice:shortcuts.context` with its items `choice:shortcuts.context/<all\|global\|library\|editView\|…>`, `check:shortcuts.showUnbound`, a row `shortcut:<command>/<context>` (a command without keys: the filter's context, `global` under All) with its key cells `shortcut:<command>/<context>/0` and `/1` and its ↺ `shortcut:<command>/<context>/restore`, the conflict line `text:shortcuts.conflict` with `button:shortcuts.replace` and `button:shortcuts.cancel`, `button:shortcuts.resetAll` and its question `text:shortcuts.resetAll` with `button:shortcuts.resetAll/confirm` and `button:shortcuts.resetAll/cancel`, and `text:shortcuts.waiting` while no library is open); M11: `dialog:keepVersion` with `button:dialog.left` and `button:dialog.right` (labelled Top and Bottom while stacked) and `check:dialog.dontAsk`, `dialog:applyChanges` with `button:dialog.apply`, `button:dialog.discard` and `check:dialog.dontAsk` (both drafts: `dontAsk`, and `then`, the action that asked, run once answered), `dialog:undoAllEdits` with `button:dialog.removeEdits`, `dialog:removeFromCard` with `button:dialog.remove` (its Cancel button is labelled Keep Files), `dialog:viewerSettings` with `button:dialog.ok`, `dialog:stopImport` with `button:dialog.stopImport` (its Cancel button is labelled Keep Importing); M13: `dialog:newPerson` with `field:dialog.name`, `field:dialog.nickname` and `button:dialog.ok`, `dialog:ignoreFaces` with `button:dialog.ignore` and `check:dialog.dontAsk`, `dialog:renamePerson` with `button:dialog.ok`, `dialog:mergePeople` with `button:dialog.merge`, `dialog:deletePerson` with `button:dialog.delete`, `dialog:faceModel` with `check:dialog.accept` and `button:dialog.download` (enabled once the box is ticked; M13.27: each file's row `text:dialog.model/<id>` with `button:dialog.page/<id>`, the download's `progress:dialog.download` with its stop `button:dialog.stop`, `button:dialog.addFile`, and for a local file `button:dialog.install`; its fields `models`, `queue`, `job`, `error`, `inspect`), `dialog:removeFaces` with `button:dialog.removeFaces` (its Cancel button is labelled Keep Faces), `dialog:writePeopleNames` with `button:dialog.write`, `dialog:removeFaceModel` with `button:dialog.remove`, `dialog:dropFaceData` with `button:dialog.delete` (its Cancel button is labelled Keep); Options ▸ People (M13.27): `check:option.people.findFaces`, `check:option.people.suggestNames`, `check:option.people.storeNamesInPhotos`, the sliders `slider:option.people.suggestionConfidence` and `slider:option.people.groupingConfidence` with their fields `…/value`, a model's row `text:option.people.model/<id>` with `button:option.people.model/<id>/download`, `/remove` and `/use`, `button:option.people.addFile`, and `text:option.people.models` when there is no model to list, `dialog:changeLocation` with `button:dialog.change`, `dialog:clearLocation` with `button:dialog.remove`), `dialog:<kind>/backdrop`, `field:dialog.<name>`, `check:dialog.<name>`, `choice:dialog.<name>`, `button:dialog.ok`, `button:dialog.cancel`, `button:dialog.<action>` (`button:dialog.undoSave`, `button:dialog.clearTray`, `button:dialog.help`, `button:dialog.continue`); M6's dialogs: Folder Properties `field:dialog.name` (renames the folder on disk, M9.20), `field:dialog.date`, `check:dialog.useOldest`, `field:dialog.place`, `field:dialog.description`, `field:dialog.music` and `button:dialog.browse` (disabled until Dalica plays sound); Adjust Date `field:dialog.date`, `field:dialog.time`, `choice:dialog.mode/<shift\|set>` (several photos); Rename Tag `field:dialog.to`; Configure Quick Tags `check:dialog.useRecent`, `field:dialog.slot1` … `field:dialog.slot10`, `button:dialog.fillCommon` |
| Folder Manager and first run (M5) | rows `choice:dialog.folder/<roothash>[/<pathhash>]`, `button:dialog.expand/…` (and `…/loading` while a listing is read), `choice:dialog.mode/<always\|once\|remove>`, `check:dialog.faces`, `button:dialog.reveal/<hash>` (the watched list), the resize grip `resize:dialog.se` (M10.25; the draft's `size` is `[width, height]`), `choice:dialog.start/<places\|choose>` (first run); stacked questions `dialog:removeFolder`, `dialog:watchDrive`, whose buttons are `dialog:<kind>/<action>` while another dialog is under them |
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

- **In the running app:** `ui.screenshot {headless: true}` paints the window's frame with this
  renderer from a CPU copy of the window's textures, running the window's passes from the logic
  tick while it draws none (§6.11). `--headless` runs the app on this renderer from the start.
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
  `--automation-allow-people`) people names or where a face is: not the commands' replies, not the
  window's (`ui.inspect`, `ui.state`, `ui.widgets`, `ui.menu.tree`), not `slideshow.list`'s focus
  (§6.4). Screenshots show whatever the window shows.
- Security-relevant changes to this channel follow `contributing.md` (state whether the change adds
  a control or documents a limitation).
