# MCP server

Adapted from LightCraft `docs/mcp.md` (revision 88ea1a8) and PhotoCraft `docs/mcp.md` (revision
59684266); MIT OR Apache-2.0.

> **Status: In progress (M15).** Built (M10.4): the server's core in `crates/mcp` (the framing,
> `initialize`, `tools/list`, `tools/call`, progress and cancellation, the limits), the shared tools
> `command_list`, `command_run`, `command_batch` and `render_preview` (§4.1), and headless mode:
> `dalica-cli mcp [--demo] [--library DIR] [--compact] [FOLDERS…]`; `doc_inspect` (M10.7); one tool
> per command and the resources (M10.15, §4.4 and §7); the organizing helpers of §4.2,
> `scan_folder` … `jobs_cancel` (M10.13); the edit, render and export helpers of §4.2 (M10.14);
> the roots and permissions of §5 and `--read-only` (M10.16: the engine's one path policy,
> `--automation-*`, the client's roots, `--no-client-roots`); connect mode and the UI tools of §4.3
> (M10.17; `dalica-cli mcp --connect` wired by M10.16); the workflow tests' harness with W02,
> W03, W08 and W09 (M10.18); W01, the smoke run of W02 and W03 through the spawned binary
> (M10.20); W04, W05, W06 and W12 (M10.19); `apply_edit` as Batch Edit, one undo step for all its
> photos, with W07 and W10 and the workflow floor at the eleven workflows M11 requires (M11.22);
> W11, the effects stacked (Sepia, Vignette, Border), and the floor at all twelve workflows (M12.20);
> `set_metadata {geo}` live, `photo_info` with the geotag and the faces, people data withheld
> without `--automation-allow-people` on every reply that carries it, W13 (geotags) and W14
> (people), and the floor at all fourteen workflows (M13.29); collages and a legacy library over
> MCP: `collage.create` and `collage.saveDraft` written only inside a write root or a watched
> folder headless, the default `<Pictures>/Dalica/Collages` included, W15 (collage) and W16
> (legacy library), and the floor at all sixteen workflows (M14.21); videos over MCP: the
> `cmd_video_*` tools headless, playing on the session's silent sound, `photo_info`'s `kind` and
> `durationMs`, the `export` tool's `videos` (`frame` or `full`), Save Frame and Export Clip written only inside a write root, W17 (video), and the
> floor at all seventeen workflows (M15.30).

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
| `--automation-read-root DIR`, `--automation-write-root DIR`, `--automation-allow-external`, `--automation-allow-people` | As for the desktop app (`control-protocol.md` §1, §5); headless mode only. Flags only: the desktop app's `DALICA_AUTOMATION_*` environment variables do not apply here. Relative folders are taken from the working folder. |
| `--no-client-roots` | Never ask the client for its roots (§5). |
| `--connect [HOST:PORT]` | Default `127.0.0.1:7990`. Loopback addresses only. |
| `--control-token-file PATH` | Connect mode: the token file; default is the app's default token file for that port (`control-protocol.md` §2.1). |

Without `--demo`, `--library` or folders, the headless session starts with an empty library;
`scan_folder` adds folders to it. The engine has no in-memory session, so an empty library, like the
demo, is a library folder in the OS temporary folder deleted on exit; folders given without
`--library` are added to such a library as Scan Once.

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
| `command_list` | `filter?`, `enabled_only?` | `{commands}`: the command registry, each with id, label, menu path, shortcut, params doc, effect, history, enabled (as `engine.commands`; `filter` matches id, label or menu path) |
| `command_run` | `id`, `params?`, `wait?` | the command's result; for a command that starts a job, the job's final entry with its `result` (as `jobs.wait`), or `{job, pending: true}` at once with `wait: false` |
| `command_batch` | `steps: [{id, params?, wait?}]`, `stop_on_error?` (true) | `{completed, failed, results: [{id, ok, result \| code + error}]}`; one undo step per command, not a transaction; ≤ 256 steps |
| `doc_inspect` | `photos?`, `section?`, `offset?`, `limit?` | the library: the JSON of `library.inspect` (`control-protocol.md` §6.4) |
| `render_preview` | `id?` (the tray's first photo), `max_side?` (1024, 16–2048) | an image of the photo with its edits, never enlarged, through the output pipeline Export uses: PNG, or JPEG when the PNG would pass 5 MiB; `{id, width, height, mimeType, recipe}`; changes nothing |

A tool whose result is not an object (a command answering a list) gives it as `{result}` in
`structuredContent`; the text block holds the value itself. In both modes a photo command run
without `ids` gets the Photo Tray's when its params doc names `ids` as required (headless: its own
tray; connect mode: the app's, from `library.inspect`; one rule for both); a command whose `ids`
are optional (Export's `ids | section | album`) is left as it is, and an empty tray gives the
command's own refusal ("Select some photos first.").

### 4.2 Dalica helpers

Photo tools take `ids` (or `id`); omitted, they act on the **Photo Tray** contents (current selection
plus held photos), as the bottom-bar buttons do.

| Tool | Arguments | Does |
|---|---|---|
| `scan_folder` | `path`, `mode?` (`always` \| `once`, default `once` headless, `always` in the app), `wait?` (true) | Add a folder to the watched folders (Folder Manager) and scan it; returns `{folder, section, photos, videos, skipped, problems}`. Progress notifications per batch of files (§6). The path must be inside a read root. |
| `query_photos` | `query?`, `section?`, `starred?`, `tags?`, `has_caption?`, `edited?`, `hidden?`, `video?`, `geotagged?`, `taken_after?`, `taken_before?`, `sort?` (`date` \| `name` \| `size` \| `modified` \| `manual`), `reverse?`, `offset?`, `limit?` (100, ≤ 1000) | Photos matching the light-table search (`query` uses the search syntax of `organizing.md` §8.1: words, `"phrases"`, `-word`, `tag:`, `camera:`) and the filters; `person:name` finds a person's photos (names and nicknames, as prefixes) only with `--automation-allow-people`: without it the search leaves names out of the match (§5). Returns `{total, photos: [{id, path, name, section, star, caption, tags, date, width, height, edited, hidden, video, geo, geotag}]}`. Changes nothing (it does not enter search mode). |
| `photo_info` | `id` | Everything Dalica knows about one photo: path, file size, dimensions, dates, EXIF summary, star, caption, tags, rotation, recipe (string and list), albums, geotag (`[lat, lon]` or `null`: the location set in Dalica, else the file's GPS) and `geo` (whether it has one), faces, hidden, held, originals backup present, unsaved edits, `video`, `kind` (`still` or `video`) and `durationMs` (a video's length, else `null`). Faces' names and rectangles only with `--automation-allow-people` (§5). |
| `select_photos` | `ids`, `section?`, `mode?` (`replace` \| `add` \| `remove`), `hold?` | Set the light-table selection (one section at a time, `ui-layout.md` §5.4) and optionally hold it in the tray. An unknown id is an error and leaves the selection unchanged. |
| `set_metadata` | `ids?`, `star?`, `caption?`, `tags_add?`, `tags_remove?`, `hidden?`, `geo?` (`[lat, lon]` or `null`), `date?`, `confirm?` | Change organizer data in **one undo step**; written to sidecars and (per the Options) into file metadata (`data-model.md` §5). `geo` is `photo.geo.set`: `null` removes the location, as Clear Location does; moving the location of more than 20 geotagged photos, or removing it from more than 5, answers `needs_confirm` until `confirm: true`. |
| `create_album` | `name`, `ids?`, `description?` | New album with these photos (default: the tray); returns `{album, section}`. |
| `list_edit_ops` | `op?` | Every edit operation: name, label, parameters with ranges and defaults, whether it changes the canvas size, its milestone and whether it is available yet (the tables of `editing.md` and `data-model.md` §3.1). |
| `get_recipe` | `id` | `{recipe: "crop(…)\|autofix()\|sepia()", ops: [{op, params}], redo: […], saved}` |
| `set_recipe` | `id`, `recipe` (string or ops list), `label?`, `drop_unknown?` (false) | Replace the photo's whole recipe in one undo step. Refuses to drop operations it cannot interpret (`legacy:…` and unknown ops from newer versions) unless `drop_unknown: true` (`data-model.md` §3.1: an unknown operation is never lost silently). |
| `apply_edit` | `ids?`, `op`, `params?` | Append one operation to each photo's recipe through Batch Edit (`edit.apply`, `editing.md` §9): one library undo step for all photos, named by the operation (`cmd_edit_undo` takes it back). Follows the edit view's rules (only the last crop and straighten count, and the same one again leaves that photo out; Tuning merges into a trailing `tuning` op, the values given replacing its values: photos whose merged entries differ are one `engine.group` of `edit.apply`s, still one step headless). An operation that does not render yet, which Batch Edit greys, is still kept in each recipe as one step of each photo's history until it renders. E.g. `{op: "autofix"}`, `{op: "crop", params: {rect: [0.08, 0.1, 0.92, 0.86]}}`, `{op: "sepia"}`, `{op: "soft_focus", params: {x: 0.5, y: 0.4, size: 0.3, amount: 0.6}}`. Rotation is `cmd_photo_rotate_cw`, not an edit op (`editing.md` §10). |
| `render_photo` | `id?`, `size?` (long edge, 1024, ≤ 2048), `format?` (`png` \| `jpeg`), `before?` (false: the unedited original), `recipe?` (preview this recipe instead of the stored one, without applying it), `path?` | **Image content** of the photo rendered by `edit::render` — the CPU oracle that thumbnails, Save and Export use — plus `{id, width, height, recipe}`. With `path` the file is written (write root) and the image is still returned. PNG results are capped at 5 MiB before encoding. Changes nothing in the library. |
| `export` | `ids?` \| `section?` \| `album?`, `dir`, `folder_name?`, `size?` (long edge px, default 1600; `0` = original size), `quality?` (`auto` \| `normal` \| `maximum` \| `minimum` \| 1–100), `numbered?`, `watermark?`, `videos?` (`frame` \| `full`), `remove_location?` | Export as the Export dialog does (`creating.md` §2): a new folder under `dir` (write root), never overwriting (`-001`), never enlarging, metadata copied; `videos` `frame` (the default: a playable clip's first frame as a picture, a clip Dalica cannot play skipped with the reason) or `full` (every clip copied byte for byte, `width` and `height` `null`). Returns `{folder, files: [{id, path, width, height, bytes}], skipped}`. Progress and cancellation per photo (§6). |
| `jobs_list` / `jobs_cancel` | — / `job?` | Background jobs (`control-protocol.md` §6.13). |

How the organizing helpers answer (M10.13): each is built from engine commands, never a second
implementation. `scan_folder` counts the photos and videos of the folders under the path as the
light table shows them (`library.inspect`), `skipped` being the files its scan could not read.
`query_photos` hands `query`, `tags` (each word of a tag as a `tag:` term), `section`, `starred`
and `video` / `geotagged: true` to `library.query`, and applies the other filters and `sort` to the
fields of every match (at most 10 000; more is `limit`, asking to narrow the query). `photo_info`
answers `{id, path, name, section, fileSize, dimensions, dates, exif, star, caption, tags,
rotation, recipe: {string, list, redo}, albums, geotag, geo, faces, hidden, held, originals,
unsavedEdits, video}`; `originals` is `null` while the photo has unsaved edits (no query says it
then); `geotag` comes from `places.list` and `faces` from `faces.list` (M13.29). `set_metadata`
with `geo` adds a `photo.geo.set` step to its group (M13.29; a step that asks first is
`needs_confirm` with its question, headless as through `engine.execute`). In
headless mode the backend adds three methods for them: `engine.group {steps, label?}` (several
library commands as one undo step), `tray.select {ids, section?, mode?, hold?}` and `tray.state`;
connect mode maps them onto the app (a grouped execute, `select.set` then `tray.hold`,
`library.inspect` → `selection` and `tray`).

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

Everything is a command, so `tools/list` also contains one tool per entry of `engine.commands` —
engine commands, plus the app's `UI_COMMANDS` (`view.edit`, `pane.tags`) in connect mode; a menu
item whose command is not registered yet (`live: false`) gets none: `cmd_` + the id with `.`
replaced by `_` (`photo.star.toggle` → `cmd_photo_star_toggle`, `photo.moveToNewFolder` →
`cmd_photo_moveToNewFolder`, `export.run` → `cmd_export_run`). The title is the label; the
description is the label, the id, the menu path (`Picture ▸ Star / Unstar`), the shortcut and the
params doc; the arguments are the command's JSON params, a loose object the command checks itself
(as `command_run`'s `params`). A call runs as `command_run` does: one undo step, a job it starts
waited for with its progress, a photo command without `ids` on the Photo Tray (§4.1).

The annotations are derived from what the registry already says, so `CommandSpec` gains no field
(`architecture.md` §3):

| The command | `readOnlyHint` | `destructiveHint` | `idempotentHint` |
|---|---|---|---|
| effect `query` | true | false | true |
| effect `library`, `files`, or a UI command's `view` | false | true when it asks first: its params doc names `"confirm"`, or the control channel asks for it anyway (Rebuild Library) | false |
| effect `external`, or no effect given | false | true | false |

`openWorldHint` is false everywhere. A test runs every command without `confirm` and fails when one
answering `needs_confirm` is not `destructiveHint`. `--compact` leaves these tools out of
`tools/list` for clients that struggle with long tool lists; `command_run` still reaches every
command. There are no hidden aliases.

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
  `-001` names. A collage (`collage.create`, `collage.saveDraft`) is written as an export is: the
  folder the engine would write it in (`path`, else the folder of the collage the room was
  reopened from, else `<Pictures>/Dalica/Collages`) must be inside a write root or a watched
  folder, or the command is refused with `path_denied` before it runs, nothing made. In-library operations (star, caption, Save with backup, move, delete to trash) need no
  root; destructive ones need `confirm: true`.
- Effects outside Dalica (`control-protocol.md` §5.6) are refused without
  `--automation-allow-external`; people names and face regions are withheld without
  `--automation-allow-people`; face descriptors are never returned. Headless, every reply of
  `engine.execute` and of each `engine.group` step goes through the engine's
  `Policy::people_reply` (`faces.list`, `faces.add`, `people.list`: names and nicknames `null`,
  `rect` and `displayed` left out, ids and counts kept), so every tool that runs a command
  (`command_run`, `command_batch`, `cmd_*`, `photo_info`'s faces) answers the same; and
  `export.defaults` of a person's section suggests its fallback name (`Export <date>`), not the
  person's, so the `export` tool's default folder names nobody. The commands themselves run as
  asked (naming a face works without the permission; its reply carries ids only), but for the
  searches: `library.query`, `search.set`, `search.suggest` and `slideshow.list` run with
  `"withholdNames": true` (`Policy::people_params`, `control-protocol.md` §6.4), so
  `query_photos {query: "person:Ana"}` and the plain word `Ana` find nothing by her name (`person`
  unavailable), while everything else matches as before and the filter by person id still holds:
  no reply tells a guessed name from an unknown one. A person's export folder is withheld as the
  control channel withholds it (`control-protocol.md` §6.13): `jobs.list`, `jobs.wait` and the
  final job entry a tool that started an export answers (`cmd_export_run`, `command_run`) give that
  job's `result.folder`, `result.name` and `result.written` `null` and its failures' reasons `null`
  (counts kept), no job gives a path into the folder it made, searches find no photo through the
  folder's name and `search.suggest` does not suggest it (`Session::job_for_automation`; M15.51).
- `--read-only` overrides all of this: nothing is written at all. Commands that write files
  (saves, exports, file operations) answer `read_only`; every other change stays in memory. The
  refusal lives in the engine's session, not in the tools (`security.md` §6.6).
- The latest `roots/list` answer replaces the client's roots, so a removed root stops granting at
  once; an error answer drops them all; only local `file://` URIs (no host, or `localhost`) count.
- **Connect mode** uses the policy of the desktop process (its flags); the MCP process's own root
  flags are ignored there, as in PhotoCraft. People data is the exception: the app withholds it
  by its policy, and the MCP process withholds it again (`Connect::allow_people`, from
  `dalica-cli mcp --connect --automation-allow-people`), so names reach the client only when both
  were started with it; without its own flag it also sends searches with names withheld.

Results that leave the machine: an MCP client may send tool results (photos rendered by
`render_photo`, captions, paths) to a remote model. That is the user's choice when they connect a
client; Dalica itself never makes a network request on behalf of a tool.

## 6. Progress and cancellation

`scan_folder`, `export`, `apply_edit` / `command_batch` steps that run as jobs, `cmd_photo_save` over
many photos and other long commands report progress when the request's `params._meta.progressToken`
is a string or number: `notifications/progress` with `progress` (photos done) and `total`, strictly
increasing, at most ten per second plus the final one. No token, no notifications. `ping` is answered
between photos.

`command_run` and `command_batch` report the progress of the jobs a command starts (the photos done
of all of them; a batch reports the steps done) while they wait. `notifications/cancelled` with the
request's id stops the job before its next photo and suppresses the response. Photos already done stay done (each written atomically), nothing is left half-written,
and the photos completed are not undone. Unknown or finished request ids are ignored. Connect mode
forwards cancellation as `jobs.cancel`.

## 7. Resources

`resources/list`, `resources/templates/list` and `resources/read` serve live JSON (`mimeType`
`application/json`), read again on every request and never cached. Each is the result of the tool of
the same content, so the two never disagree:

| URI | Content |
|---|---|
| `dalica://library` | the library (`doc_inspect` without photos) |
| `dalica://commands` | the command registry (`command_list`) |
| `dalica://selection` | `{tray: {items, …}, total, photos}`: the Photo Tray (the photos photo tools act on without `ids`) and a summary of each of its first 1 000 photos (`id`, `folder`, `name`, `path`, `star`, `caption`, `tags`, `hidden`, `effectiveDate`, `displayed`, `edited`); with the app attached also its `selection` and its whole `tray` (held photos, `library.inspect`) |
| `dalica://photo/{id}` | `photo_info` for one photo (resource template); until that tool exists (M10.13), the photo's `photo.inspect` entry |
| `dalica://edit-ops` | `list_edit_ops`; until that tool exists (M10.14), `{ops: [{op}]}` with each operation's name |

An unknown URI, or a photo URI whose id is not a photo, is `-32002`. No resource carries people data:
a `faces` or `people` field is dropped.

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

- Location: `crates/mcp/tests/workflows/`, one module per workflow, named `wNN_<slug>`: a test
  binary of its own (`cargo test -p dalica-mcp --test workflows`), which `cargo xtask stats` runs
  alone. `harness.rs` gives each workflow a temporary directory of `scenes` folders and a library,
  and an agent that calls tools by name; deletes land in the stand-in trash of `dalica-fs`, never
  the user's. Headless mode has no View menu: a workflow that needs Hidden Pictures (W09) stores it
  in the library's `settings.json` between two sessions, as the app keeps it.
- Transport: each test drives the server through the real JSON-RPC framing (in-process pipes); a
  smoke subset also spawns `dalica-cli mcp` as a process with real stdio, and runs once in connect
  mode against a headless app with a control server, to prove both modes agree. The stdio smoke
  run (`apps/dalica-cli/tests/mcp_process.rs`) takes W02's and W03's steps through
  `dalica-cli mcp --library DIR --automation-read-root DIR --no-client-roots`, checking the
  sidecars and file bytes on disk and that standard output carries only JSON-RPC messages.
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
| W01 | **Organize and export** (the `process.md` §5 example): `scan_folder` 200 photos → star 5 → `create_album` from them → caption 3 → crop and Auto Fix one → `export` the album at 1600 px → check 5 JPEGs, long edge ≤ 1600, captions in XMP/IPTC, crop visible in the output, originals byte-identical | M10 (uses M1–M9 features) |
| W02 | **Tray across folders**: select 2 in folder A → hold → select 3 in folder B → tray holds 5 → Rotate Clockwise on the tray → sidecars of both folders record `rotate(90)` → undo restores all 5 in one step | M6 |
| W03 | **Save, Undo Save, Revert**: edit → `cmd_photo_save {confirm}` → original in `.dalica-originals/` → Undo Save restores it with the edits unsaved → Save → Revert returns the exact original bytes (`data-model.md` §10.9) | M8 |
| W04 | **Rebuild loses nothing**: star, caption, tag, hide, edit, album → `library.rebuild {confirm}` → `doc_inspect` and `photo_info` equal before and after (`data-model.md` §10.1) | M9 |
| W05 | **Move carries data**: drag-equivalent move of 3 starred, captioned, saved photos to another folder → their sidecar entries and originals backups moved, nothing left behind, undo moves them back (§10.3) | M9 |
| W06 | **Search and save results**: tag 10 photos, caption 2 with "tram" → `query_photos {query: "tram"}` and `tag:` queries return the expected ids → save the search as an album | M9 |
| W07 | **Batch edit is one step**: `apply_edit {op: "autofix"}` on 20 photos → 20 recipes end in `autofix()` → one `cmd_edit_undo` removes all 20 | M11 |
| W08 | **Foreign metadata survives**: photos carrying other applications' XMP fields → caption and tags → those fields byte-identical, pixels untouched, mtime unchanged by default (`data-model.md` §10.8) | M6 |
| W09 | **Hidden photos**: hide 4 → they leave the light table and `query_photos` → `view.hiddenPictures` shows them → unhide | M6 |
| W10 | **Import with duplicates**: import a card-like folder twice → second import skips duplicates and reports them; destination naming as configured | M11 |
| W11 | **Effects stack**: `apply_edit` sepia then vignette then border → recipe order kept → `render_photo` size grows for Border → render fingerprint within tolerance (`architecture.md` §11); `list_edit_ops` lists the 27 effects, each `available` when its module renders | M12 |
| W12 | **Destructive commands ask**: `cmd_photo_delete` without `confirm` → `needs_confirm`, file still there → with `confirm` → file in the OS trash (a test trash directory), sidecar section kept in the undo record | M9 |
| W13 | **Geotags**: `set_metadata {geo}` on 3 photos of 5 (one undo step) → GPS in each of their files read back, none in the others → `photo_info` and `query_photos {geotagged: true}` report them → `set_metadata {geo: null}` → GPS gone from the files → one `cmd_edit_undo` restores it in the library and the files | M13 |
| W14 | **People**, with `--automation-allow-people` and Store people names in photos on: faces planted in 5 photos (the folder's sidecar) → 4 named Ana by one `cmd_faces_name` → `query_photos {query: "person:Ana"}` returns those 4 → the MWG regions named Ana in their files, none in the others → a session without the permission reads names `null` and no rectangles (`cmd_people_list`, `cmd_faces_list`, `photo_info`), counts kept, and a person's export folder not named after them | M13 |
| W15 | **Collage**: `cmd_create_collage` on 8 photos → `cmd_collage_set {style: "grid"}` → `cmd_collage_create` with a path in the write root (`<Pictures>/Dalica/Collages`) → a JPEG of the page's ratio at 3000 px on its long side and its `.dalica-collage` recipe beside it (grid, the 8 clips), the photos untouched → the folder, scanned when the job ends, is in the projects group, marked `collages`, and `photo_info` lists the collage there → `cmd_collage_edit` reopens the room with the 8 clips | M14 |
| W16 | **Legacy library**: `scan_folder` on a folder with only the previous organizer's sidecar (written by the test) → `photo_info` shows its stars, captions, a recipe with a `legacy:` entry, the album and the person (with `--automation-allow-people`), nothing written → `set_metadata` stars one → `.dalica.json` holds every photo, the legacy file and the photos byte-identical → `cmd_legacy_inspect` says `both`. Headless applies the scan's legacy results and the first write's report through the session's poll, as the app does each frame | M14 |
| W17 | **Video**: `scan_folder` on a folder of two photos, an H.264 clip and an MPEG-4 Part 2 clip (made by the test) → `photo_info` shows each clip's `kind` (`video`), `durationMs` and date → `cmd_video_inspect`: the H.264 clip `playable`, the other not, with the reason ("This video can't be played (MPEG-4 Part 2)") → the H.264 clip plays headless on the silent sound → `cmd_edit_recipe_set` with `trim(500,1500)` → `cmd_video_seek {ms: 1000}` and `cmd_video_saveFrame` → a JPEG of the clip's size in `<Pictures>/Dalica/Video Frames`, a folder in the projects group → the other clip opens with the reason and its Save Frame is refused with it, nothing written → `cmd_export_run` of the folder as first frames writes three JPEGs and counts the other clip as skipped with the reason → `export {videos: "full"}` copies both clips byte for byte; the clips only read. With no video codec passing `docs/video-decoders.md`, W17 would stop after `cmd_video_inspect` and the export's count | M15 |

New user-visible features add a workflow when they add a user task; a bug report that reproduces as
a workflow lands with it (it fails before the fix).

## 10. Limits

As the control channel (`control-protocol.md` §4), plus: `render_photo` / `render_preview` long edge
≤ 2048 and PNG ≤ 5 MiB; tool results ≤ 8 MiB encoded (a batch stops at the step that would exceed
it, and says so); `query_photos` `limit` ≤ 1000; `command_batch` ≤ 256 steps. MCP stdio input lines
are capped at 1 MiB like the control channel; an over-long line gets `-32700` and is skipped.
