# Dalica documentation

How Dalica should be built and how it behaves. Everything here is written in our own words;
private reference research is kept out of the repository (see `AGENTS.md`).

Start with [`product.md`](product.md), then [`ui-layout.md`](ui-layout.md).

| Doc | Read it when you… |
|---|---|
| [product.md](product.md) | want to know what Dalica is, its principles and scope tiers |
| [ui-layout.md](ui-layout.md) | touch any part of the main window (regions, sizes, selection model, tray, status line) |
| [organizing.md](organizing.md) | work on folders, scanning, import, albums, stars, tags, captions, people, places, search, file operations |
| [editing.md](editing.md) | work on the edit view, tools, tuning, effects, undo, batch edit, video trim |
| [viewing.md](viewing.md) | work on slideshows, quick preview, the standalone viewer, video playback |
| [creating.md](creating.md) | work on export and collage |
| [menus-and-shortcuts.md](menus-and-shortcuts.md) | add a command, menu item, shortcut, context-menu item or option |
| [data-model.md](data-model.md) | read or write anything on disk (sidecars, metadata, originals, index, file operations) |
| [standards/never-crash.md](standards/never-crash.md) | write any non-test code (the never-crash standard) |
| [security.md](security.md) | touch a parser, file writes or paths, the control channel, MCP, a dependency or a release; handle a vulnerability report |
| [design-system.md](design-system.md) | draw anything (colours, type, metrics, icons, widgets) |
| [architecture.md](architecture.md) | add a crate, a command, a thread, a cache; need the automation hooks |
| [control-protocol.md](control-protocol.md) | drive or screenshot the app from a script or agent, add a control method or widget id, use headless snapshots (planned, M2) |
| [mcp.md](mcp.md) | connect an AI agent over MCP, add an MCP tool, write a workflow acceptance test (planned, M21) |
| [showcase.md](showcase.md) | make or update README screenshots (scripted, procedural or public-domain photos only) |
| [development.md](development.md) | build, run, test, benchmark, use xtask |
| [process.md](process.md) | want to know how we work with AI agents, clean-room rules, reviews, verification |
| [parity.md](parity.md) | pick work or record progress |
| [roadmap.md](roadmap.md) | need milestones, task ids, the honest status and reserved decisions |
| [status.md](status.md) | start or end a work session |
| [task-board.md](task-board.md) | find, claim, submit or merge a task; run several agents at once |
| [contributing.md](contributing.md) | open a pull request: rules, the "Adding a command" checklist, PR expectations |
| [contributors.md](contributors.md) | want to know how people and AI models are credited in the About window |
| [assets.md](assets.md) | add an image, icon, font, sound, screenshot or test photo; touch the app icon |
| [localization.md](localization.md) | write user-facing text, or add or maintain a translation |
| [releasing.md](releasing.md) | cut a release, touch packaging or signing (planned, M11) |
| [whats-new.md](whats-new.md) | ship a user-visible change (the user-facing changelog) |
| [formats.md](formats.md) | touch decoding, thumbnails, RAW previews, HEIF, or the decode limits |
| [xmp-interop.md](xmp-interop.md) | read or write XMP/IPTC/EXIF in photo files or `.xmp` sidecars (captions, tags, ratings, GPS, dates, face regions) |
| [faces.md](faces.md) | work on face detection, recognition, models, grouping or suggestions (planned, M29) |

Conventions in these docs:
- Sizes are in points at 100 % UI scale; "W" is the window width.
- `Ctrl` means Cmd on macOS unless stated.
- "The reference organizer" means the application whose workflows Dalica re-creates.
- Items marked "Dalica addition" go beyond the reference behaviour on purpose.
