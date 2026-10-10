# MCP server

Adapted from LightCraft `docs/mcp.md` (revision 88ea1a8) and PhotoCraft `docs/mcp.md` (revision
59684266); MIT OR Apache-2.0.

> **Status: Planned (M4).** Nothing here exists yet. The server, its helper tools and the workflow
> acceptance-test harness land in milestone **M4** (see [`roadmap.md`](roadmap.md)); individual
> workflows are added as the features they exercise land (§9). Connect mode needs the control channel
> of [`control-protocol.md`](control-protocol.md) (M0).

`dalica-cli mcp` exposes Dalica to AI agents through the
[Model Context Protocol](https://modelcontextprotocol.io): newline-delimited JSON-RPC 2.0 over stdio
(protocol revision `2025-06-18`; `2025-03-26` and `2024-11-05` clients are accepted). The server
lives in `crates/mcp` (`dalica-mcp`, layer L5) and is hand-written like LightCraft's: no async
runtime, no C dependencies. It is a thin layer over the command registry (`architecture.md` §3):
everything an agent can do here, a user can do from a menu, and vice versa.

## 1. Modes

| Mode | Command | What it drives |
|---|---|---|
| **Headless** (default) | `dalica-cli mcp [--demo] [--library DIR] [FOLDERS…]` | An in-process engine `Session` with no window: scan, organize, edit, render and export. |
| **Connect** | `dalica-cli mcp --connect [HOST:PORT]` | A running desktop app started with `dalica --control <port>`, through its loopback JSON-lines control channel. Adds the UI tools (§4.3). |

Options:

| Option | Meaning |
|---|---|
| `--demo` | The procedural demo library (`dalica-scenes`), written as real JPEG files with sidecars into a temporary folder deleted on exit, so file operations, sidecars and metadata behave as on a real disk. Deterministic ids (`control-protocol.md` §5.4). |
| `--library DIR` | Open (or create) a persistent Dalica library directory (`data-model.md` §4). A library is open in one process at a time: while the desktop app has it, this fails with "This library is already open in Dalica (process N)"; use connect mode instead. |
| `FOLDERS…` | Watch these folders for this session (Scan Once, in memory unless `--library` is given). They become read roots (§5). |
| `--read-only` | Nothing is written to disk: sidecar, metadata and index writes stay in memory, file operations and exports are refused. For exploring a real library safely. |
| `--compact` | List only the helper tools, not one tool per command (§4.4). |
| `--automation-read-root DIR`, `--automation-write-root DIR`, `--automation-allow-external`, `--automation-allow-people` | As for the desktop app (`control-protocol.md` §1, §5); headless mode only. |
| `--connect [HOST:PORT]` | Default `127.0.0.1:7990`. Loopback addresses only. |
| `--control-token-file PATH` | Connect mode: the token file; default is the app's default token file for that port (`control-protocol.md` §2.1). |

Without `--demo`, `--library` or folders, the headless session starts with an empty in-memory
library; `scan_folder` adds folders to it.

**Connect mode** starts even when the app is not running yet and connects on the first call. It keeps
one authenticated connection; if a request fails in transit (including a reply timeout) the
connection is dropped and the tool result says the operation **may have completed** — it never
resends a request by itself. The next call reconnects (the app may have restarted). Reply lines with
a mismatched `id` are skipped.

Logs go to standard error; standard output carries only protocol messages.

## 2. Wiring it into a client

Build once: `cargo build --release -p dalica-cli` (binary `target/release/dalica-cli`).

### Claude Code

```sh
# headless, procedural demo library (safe to experiment with)
claude mcp add dalica -- "$PWD/target/release/dalica-cli" mcp --demo

# headless, a folder of test photos, outputs allowed under target/mcp-out
claude mcp add dalica-corpus -- "$PWD/target/release/dalica-cli" mcp "$PWD/corpus/photos" \
    --automation-write-root "$PWD/target/mcp-out"

# drive the running desktop app (start it with `dalica --control 7990 --demo`)
claude mcp add dalica-app -- "$PWD/target/release/dalica-cli" mcp --connect 127.0.0.1:7990
```

Or a project-scoped `.mcp.json` (never commit one that names a personal library):

```json
{
  "mcpServers": {
    "dalica": {
      "command": "/path/to/dalica/target/release/dalica-cli",
      "args": ["mcp", "--connect", "127.0.0.1:7990"]
    }
  }
}
```

### Other clients

Every stdio MCP client takes the same shape: a `command` plus `args` (for example
`claude_desktop_config.json` with `"args": ["mcp", "--demo"]`). During development you can point the
client at `cargo run --release -p dalica-cli -- mcp` with `"cwd"` set to the repository, at the cost
of a slower start. Parallel agents in connect mode each use their own app instance and port
(18000–19999).

## 3. Server behaviour

- `initialize` returns `serverInfo {name: "dalica", version}`, capabilities `tools` (with
  `listChanged` in connect mode, where UI commands come and go with the app), `resources`, and an
  `instructions` string: how ids work, that photo tools default to the Photo Tray, that destructive
  commands need `confirm: true`, and how to find tools (`command_list`).
- Every tool has a `title`, an `inputSchema`, an `outputSchema` where the result shape is fixed, and
  all four annotation hints (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`).
  `openWorldHint` is false everywhere: Dalica tools never reach the network (`control-protocol.md`
  §5.6). Hints describe; they never grant permission.
- Helper tools validate their arguments strictly: an unknown key is JSON-RPC `-32602` naming it and
  the accepted keys, before anything runs. Generated `cmd_*` tools and nested command params are
  validated by the command itself (the registry has prose params docs, not machine schemas).
- Failures of the operation (unknown photo, nothing in the tray, disabled command, path denied, app
  not reachable) are tool results with `isError: true` and the control-protocol `code` and message,
  so the model can read and correct them. Protocol errors use the standard codes: `-32700` parse
  (with `id: null`; the next line is still read), `-32600` invalid request, `-32601` method not
  found, `-32602` invalid params, `-32002` resource not found.
- An escaped panic in a tool becomes `isError: true` (`code: "internal"`) and the session keeps
  serving (`AGENTS.md` §3).
- Results carry both `structuredContent` and a JSON `text` content block. Image tools add an `image`
  content block.
- With a persistent library, a command whose change cannot be written to disk returns `isError: true`
  with `code: "unsaved"` ("saved in memory but not written to disk: …; Dalica will retry"); the
  change stays applied and queued (`control-protocol.md` §5.8).

## 4. Tools

### 4.1 Shared core tools (all Craft apps)

The same five names as LightCraft and PhotoCraft, so an agent that knows one Craft app knows where to
start in another. Listed in full and compact mode.

| Tool | Arguments | Result |
|---|---|---|
| `command_list` | `filter?`, `enabled_only?` | the command registry: id, label, menu path, shortcut, params doc, enabled, checked (as `engine.commands`) |
| `command_run` | `id`, `params?`, `wait?` | the command's result; `{job, pending: true}` for an unfinished job |
| `command_batch` | `steps: [{id, params?, wait?}]`, `stop_on_error?` (true) | `{completed, failed, results}`; one undo step per command, not a transaction; ≤ 256 steps |
| `doc_inspect` | `photos?`, `section?`, `offset?`, `limit?` | the library: the JSON of `library.inspect` (`control-protocol.md` §6.4) |
| `render_preview` | `id?`, `max_side?` (1024, ≤ 2048) | an image of the photo with its edits; changes nothing |

### 4.2 Dalica helpers

Photo tools take `ids` (or `id`); omitted, they act on the **Photo Tray** contents (current selection
plus held photos), as the bottom-bar buttons do.

| Tool | Arguments | Does |
|---|---|---|
| `scan_folder` | `path`, `mode?` (`always` \| `once`, default `once` headless, `always` in the app), `wait?` (true) | Add a folder to the watched folders (Folder Manager) and scan it; returns `{folder, section, photos, videos, skipped, problems}`. Progress notifications per batch of files (§6). The path must be inside a read root. |
| `query_photos` | `query?`, `section?`, `starred?`, `tags?`, `has_caption?`, `edited?`, `hidden?`, `video?`, `geotagged?`, `taken_after?`, `taken_before?`, `sort?` (`date` \| `name` \| `size` \| `modified` \| `manual`), `reverse?`, `offset?`, `limit?` (100, ≤ 1000) | Photos matching the light-table search (`query` uses the search syntax of `organizing.md` §8.1: words, `"phrases"`, `-word`, `tag:`, `camera:`) and the filters. Returns `{total, photos: [{id, path, name, section, star, caption, tags, date, width, height, edited, hidden, video}]}`. Changes nothing (it does not enter search mode). |
| `photo_info` | `id` | Everything Dalica knows about one photo: path, file size, dimensions, dates, EXIF summary, star, caption, tags, rotation, recipe (string and list), albums, geotag, hidden, held, originals backup present, unsaved edits. Faces only with `--automation-allow-people`. |
| `select_photos` | `ids`, `section?`, `mode?` (`replace` \| `add` \| `remove`), `hold?` | Set the light-table selection (one section at a time, `ui-layout.md` §5.4) and optionally hold it in the tray. An unknown id is an error and leaves the selection unchanged. |
| `set_metadata` | `ids?`, `star?`, `caption?`, `tags_add?`, `tags_remove?`, `hidden?`, `geo?` (`[lat, lon]` or `null`), `date?` | Change organizer data in **one undo step**; written to sidecars and (per the Options) into file metadata (`data-model.md` §5). |
| `create_album` | `name`, `ids?`, `description?` | New album with these photos (default: the tray); returns `{album, section}`. |
| `list_edit_ops` | `op?` | Every edit operation: name, label, parameters with ranges and defaults, whether it changes the canvas size, its milestone and whether it is available yet (the tables of `editing.md` and `data-model.md` §3.1). |
| `get_recipe` | `id` | `{recipe: "crop(…)\|autofix()\|sepia()", ops: [{op, params}], redo: […], saved}` |
| `set_recipe` | `id`, `recipe` (string or ops list), `label?`, `drop_unknown?` (false) | Replace the photo's whole recipe in one undo step. Refuses to drop operations it cannot interpret (`legacy:…` and unknown ops from newer versions) unless `drop_unknown: true` (`data-model.md` §3.1: an unknown operation is never lost silently). |
| `apply_edit` | `ids?`, `op`, `params?` | Append one operation to each photo's recipe, like Batch Edit (`editing.md` §9): one undo step for all photos. Follows the edit view's rules (only the last crop counts; Tuning merges into a trailing `tuning` op). E.g. `{op: "autofix"}`, `{op: "crop", params: {rect: [0.08, 0.1, 0.92, 0.86]}}`, `{op: "sepia"}`, `{op: "soft_focus", params: {x: 0.5, y: 0.4, size: 0.3, amount: 0.6}}`. Rotation is `cmd_photo_rotate_cw`, not an edit op (`editing.md` §10). |
| `render_photo` | `id?`, `size?` (long edge, 1024, ≤ 2048), `format?` (`png` \| `jpeg`), `before?` (false: the unedited original), `recipe?` (preview this recipe instead of the stored one, without applying it), `path?` | **Image content** of the photo rendered by `edit::render` — the CPU oracle that thumbnails, Save and Export use — plus `{id, width, height, recipe}`. With `path` the file is written (write root) and the image is still returned. PNG results are capped at 5 MiB before encoding. Changes nothing in the library. |
| `export` | `ids?` \| `section?` \| `album?`, `dir`, `folder_name?`, `size?` (long edge px, default 1600; `0` = original size), `quality?` (`auto` \| `normal` \| `maximum` \| `minimum` \| 1–100), `numbered?`, `watermark?`, `videos?` (`frame` \| `full`), `remove_location?` | Export as the Export dialog does (`creating.md` §2): a new folder under `dir` (write root), never overwriting (`-001`), never enlarging, metadata copied. Returns `{folder, files: [{id, path, width, height, bytes}], skipped}`. Progress and cancellation per photo (§6). |
| `jobs_list` / `jobs_cancel` | — / `job?` | Background jobs (`control-protocol.md` §6.13). |

Saving edits into the original files (`cmd_photo_save`), reverting, deleting and moving are reached
through their command tools and need `confirm: true` where the UI asks first
(`control-protocol.md` §5.5).

### 4.3 Connect mode only

| Tool | Arguments | Control method |
|---|---|---|
| `screenshot` | `max_size?`, `headless?`, `widget?`, `path?` | `ui.screenshot`, returned as **image content** after a settle (downscaled to `max_size` long edge, default 1600) |
| `inspect_ui` | `reset_perf?` | `ui.inspect` (view state, regions, popups, dialogs, perf counters) |
| `list_widgets` | `filter?`, `region?` | `ui.widgets` |
| `click` | `widget` \| `x, y`, `count?`, `button?`, `ctrl?`, `shift?`, `alt?`, `cmd?` | `ui.clickWidget` / `ui.pointer` |
| `drag` | `widget`, `to_widget` \| `to_x, to_y`, `ctrl?` | `ui.dragWidget` |
| `press_key` | `key` \| `combo`, `state?`, modifiers | `ui.key` |
| `type_text` | `text` | `ui.type` |
| `scroll` | `dx?`, `dy?`, `widget?`, `ctrl?` | `ui.scroll` |
| `menu_invoke` | `path` \| `id`, `params?` | `ui.menu.invoke` |
| `settle` | `timeout_ms?`, `jobs?` | `ui.settle` |
| `control_call` | `method`, `params?` | any control method, passed through unchanged |

In connect mode the helper tools of §4.2 are forwarded to the app (as `engine.execute` of the
matching engine commands), so they act on what the user sees; `render_photo` and `render_preview`
render through the app's session. In headless mode the tools of this table return `isError: true`
explaining how to start the app with `--control` and use `--connect`.

### 4.4 One tool per command

Everything is a command, so `tools/list` also contains one tool per registry entry — engine
commands, plus the app's `UI_COMMANDS` (`view.edit`, `pane.tags`) in connect mode: `cmd_` + the id
with `.` replaced by `_` (`photo.star.toggle` → `cmd_photo_star_toggle`, `photo.moveToNewFolder` →
`cmd_photo_moveToNewFolder`, `output.export` → `cmd_output_export`). The description is the label,
menu path, shortcut and params doc; the arguments are the command's JSON params. Annotations come
from the command's effect class — query, edit, files, destructive — so a query is `readOnlyHint` and
a delete is `destructiveHint`; a command not yet classified is conservatively marked as a
non-idempotent edit. (The effect class is a planned field of `CommandSpec`; `architecture.md` §3 is
updated when it lands.) `--compact` leaves these tools out for clients that struggle with long tool
lists; `command_run` still reaches every command. There are no hidden aliases.

## 5. Roots and permissions

The MCP server applies the same path policy as the control channel (`control-protocol.md` §5.7),
with one addition: **client roots**.

- **Read roots** (folders that may be added or read): `--automation-read-root` flags, folders given
  on the command line, the demo folder, and the client's MCP roots — when the client declares the
  `roots` capability, the server calls `roots/list` after `initialize` and on
  `notifications/roots/list_changed`, and treats each `file://` root as a read root.
  `--no-client-roots` ignores them.
- **Write roots** (where new files may be created: exports, renders, screenshots):
  `--automation-write-root` flags only; client roots are never write roots. Without a write root
  the defaults are the OS temporary directory and `<cache>/Dalica/automation/`.
- Never inside a watched folder or `.dalica-originals/`; never over an existing file; exports use
  `-001` names. In-library operations (star, caption, Save with backup, move, delete to trash) need no
  root; destructive ones need `confirm: true`.
- Effects outside Dalica (`control-protocol.md` §5.6) are refused without
  `--automation-allow-external`; people names and face regions are withheld without
  `--automation-allow-people`; face descriptors are never returned.
- `--read-only` overrides all of this: nothing is written at all.
- **Connect mode** uses the policy of the desktop process (its flags); the MCP process's own root
  flags are ignored there, as in PhotoCraft.

Results that leave the machine: an MCP client may send tool results (photos rendered by
`render_photo`, captions, paths) to a remote model. That is the user's choice when they connect a
client; Dalica itself never makes a network request on behalf of a tool.

## 6. Progress and cancellation

`scan_folder`, `export`, `apply_edit` / `command_batch` steps that run as jobs, `cmd_photo_save` over
many photos and other long commands report progress when the request's `params._meta.progressToken`
is a string or number: `notifications/progress` with `progress` (photos done) and `total`, strictly
increasing, at most ten per second plus the final one. No token, no notifications. `ping` is answered
between photos.

`notifications/cancelled` with the request's id stops the job before its next photo and suppresses
the response. Photos already done stay done (each written atomically), nothing is left half-written,
and the photos completed are not undone. Unknown or finished request ids are ignored. Connect mode
forwards cancellation as `jobs.cancel`.

## 7. Resources

`resources/list` / `resources/read` serve live JSON (never cached):

| URI | Content |
|---|---|
| `dalica://library` | the library (`doc_inspect` without photos) |
| `dalica://commands` | the command registry (`command_list`) |
| `dalica://selection` | the selection and tray with photo summaries |
| `dalica://photo/{id}` | `photo_info` for one photo (resource template) |
| `dalica://edit-ops` | `list_edit_ops` |

## 8. Example session

```text
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"me","version":"1"}}}
← {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{},"resources":{}},"serverInfo":{"name":"dalica","version":"0.1.0"},"instructions":"…"}}
→ {"jsonrpc":"2.0","method":"notifications/initialized"}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"query_photos","arguments":{"query":"lisbon","limit":3}}}
← {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{…}"}],"isError":false,"structuredContent":{"total":48,"photos":[{"id":1,"name":"IMG_0001.jpg",…}]}}}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"apply_edit","arguments":{"ids":[1],"op":"autofix"}}}
→ {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"render_photo","arguments":{"id":1,"size":768}}}
← {"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"image","data":"iVBORw0…","mimeType":"image/png"},{"type":"text","text":"{\"id\":1,\"width\":768,\"height\":512,\"recipe\":\"autofix()\"}"}],"isError":false}}
```

### Without MCP: `dalica-cli run`

For agents that prefer a shell: run a chain of commands in one process and read one JSON line per
command (`{"command", "ok", "result" | "code" + "error", "ms"}`; non-zero exit status on failure). A
word without `=` starts the next command; `key=value` values are JSON when they parse, else strings.

```sh
dalica-cli run --demo select.all photo.star.toggle library.inspect
dalica-cli run --library ~/dalica-test-lib 'photo.caption.set' 'ids=[3]' caption="Tram 28"
dalica-cli run --connect 127.0.0.1:18042 --script steps.jsonl --keep-going   # the running app, + ui.* methods
dalica-cli commands [--json]                                                 # the registry
```

## 9. Workflow acceptance tests

`docs/process.md` §5 makes the **pass rate of workflow acceptance tests** the headline parity number:
scripted end-to-end user tasks run **over MCP** against the real engine, checked only through public
state (`doc_inspect`, `photo_info`, `query_photos`) and the files on disk — never through internals.

- Location: `crates/mcp/tests/workflows.rs`, one test per workflow, named `wNN_<slug>`.
- Transport: each test drives the server through the real JSON-RPC framing (in-process pipes); a
  smoke subset also spawns `dalica-cli mcp` as a process with real stdio, and runs once in connect
  mode against a headless app with a control server, to prove both modes agree.
- Photos: procedural `dalica-scenes` folders generated into a temporary directory (deterministic, so
  the tests run in plain `cargo test` and CI). With the `corpus` feature the same workflows also run
  on CC0 corpus folders (`development.md`, corpus); they never use personal photos.
- Checks include the bytes on disk: sidecar contents, XMP/IPTC read back by the independent reader
  in `dalica-meta` tests, exported files' size, dimensions and metadata, originals untouched (sha256
  before/after) unless the workflow saves.
- A workflow whose feature is not built yet is listed with `#[ignore = "M<n>: <feature>"]`, so the
  pass rate is honest: `cargo xtask stats` reports *passing / total* and `docs/status.md` → Metrics
  records it. A floor test keeps the number of passing workflows from falling.

Initial workflows (each becomes required in the milestone shown):

| Id | Workflow | Milestone |
|---|---|---|
| W01 | **Organize and export** (the `process.md` §5 example): `scan_folder` 200 photos → star 5 → `create_album` from them → caption 3 → crop and Auto Fix one → `export` the album at 1600 px → check 5 JPEGs, long edge ≤ 1600, captions in XMP/IPTC, crop visible in the output, originals byte-identical | M4 (uses M1–M3 features) |
| W02 | **Tray across folders**: select 2 in folder A → hold → select 3 in folder B → tray holds 5 → Rotate Clockwise on the tray → sidecars of both folders record `rotate(90)` → undo restores all 5 in one step | M1 |
| W03 | **Save, Undo Save, Revert**: edit → `cmd_photo_save {confirm}` → original in `.dalica-originals/` → Undo Save restores it with the edits unsaved → Save → Revert returns the exact original bytes (`data-model.md` §10.9) | M2 |
| W04 | **Rebuild loses nothing**: star, caption, tag, hide, edit, album → `library.rebuild {confirm}` → `doc_inspect` and `photo_info` equal before and after (`data-model.md` §10.1) | M3 |
| W05 | **Move carries data**: drag-equivalent move of 3 starred, captioned, saved photos to another folder → their sidecar entries and originals backups moved, nothing left behind, undo moves them back (§10.3) | M3 |
| W06 | **Search and save results**: tag 10 photos, caption 2 with "tram" → `query_photos {query: "tram"}` and `tag:` queries return the expected ids → save the search as an album | M3 |
| W07 | **Batch edit is one step**: `apply_edit {op: "autofix"}` on 20 photos → 20 recipes end in `autofix()` → one `cmd_edit_undo` removes all 20 | M3 |
| W08 | **Foreign metadata survives**: photos carrying other applications' XMP fields → caption and tags → those fields byte-identical, pixels untouched, mtime unchanged by default (`data-model.md` §10.8) | M2 |
| W09 | **Hidden photos**: hide 4 → they leave the light table and `query_photos` → `view.hiddenPictures` shows them → unhide | M1 |
| W10 | **Import with duplicates**: import a card-like folder twice → second import skips duplicates and reports them; destination naming as configured | M4 |
| W11 | **Effects stack**: `apply_edit` sepia then vignette then border → recipe order kept → `render_photo` size grows for Border → golden render within tolerance | M5 |
| W12 | **Destructive commands ask**: `cmd_photo_delete` without `confirm` → `needs_confirm`, file still there → with `confirm` → file in the OS trash (a test trash directory), sidecar section kept in the undo record | M1 |

New user-visible features add a workflow when they add a user task; a bug report that reproduces as
a workflow lands with it (it fails before the fix).

## 10. Limits

As the control channel (`control-protocol.md` §4), plus: `render_photo` / `render_preview` long edge
≤ 2048 and PNG ≤ 5 MiB; tool results ≤ 8 MiB encoded (a batch stops at the step that would exceed
it, and says so); `query_photos` `limit` ≤ 1000; `command_batch` ≤ 256 steps. MCP stdio input lines
are capped at 1 MiB like the control channel; an over-long line gets `-32700` and is skipped.
