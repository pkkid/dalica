# Dalica

**A fast, local-first photo organizer with simple editing, written in pure Rust.**

Dalica shows every photo in the folders you choose in one continuous, scrollable light table. You
can star, caption, tag and album your photos, name the people in them and put them on a map. Fix a
photo in one click, play with effects, and make slideshows, collages and exports. There is
no library to import into, no account, and nothing touches your original files unless you
explicitly save.

Dalica faithfully re-creates the layout and workflows of a classic desktop photo organizer. Its
look is the muted dark grey of the [Craft apps](https://github.com/storytold/lightcraft)
(LightCraft, PhotoCraft), so it sits naturally beside them.

> **Status: investigation stage.** No code yet. The specs in [`docs/`](docs/) describe what
> Dalica will be; [`docs/roadmap.md`](docs/roadmap.md) says where we are.

## How it works

- **Your folders are the library.** Dalica watches the folders you pick (Tools → Folder Manager).
  New photos appear on their own, and moving or renaming in Dalica moves or renames on disk.
- **One light table.** Every folder and album is a section in one long, scrolling view. Clicking a
  folder on the left jumps to it and selects it.
- **The Photo Tray** gathers photos from anywhere (Hold). The big buttons at the bottom (Export,
  Collage, Slideshow) act on what is in the tray.
- **Edits never destroy anything.** Crop, Auto Fix, tuning and 27 effects are stored as a recipe
  next to your photo. The original pixels change only when you Save, and even then the original is
  kept and can be restored.
- **Your work is portable.** Stars, captions, tags, albums, faces and edits are written to small,
  readable sidecar files in each folder and to standard metadata. The central index is only a
  cache.
- **Private.** No account, no telemetry. Face recognition runs on your machine.
- **Built for agents.** Every action is a command with a stable id, available from the UI, the
  keyboard, a CLI, a local control channel and an MCP server. AI agents can drive the real app
  and see the result.

## Documentation

| Doc | What it covers |
|---|---|
| [docs/product.md](docs/product.md) | what Dalica is, principles, scope |
| [docs/ui-layout.md](docs/ui-layout.md) | the window: regions, sizes, behaviour |
| [docs/organizing.md](docs/organizing.md) | folders, import, albums, stars, tags, people, places, search, file operations |
| [docs/editing.md](docs/editing.md) | edit view, tools, tuning, effects, undo, batch edit |
| [docs/viewing.md](docs/viewing.md) | slideshow, full-screen preview, photo viewer, video |
| [docs/creating.md](docs/creating.md) | export and collage |
| [docs/menus-and-shortcuts.md](docs/menus-and-shortcuts.md) | full menu tree, keyboard shortcuts, context menus, preferences |
| [docs/data-model.md](docs/data-model.md) | what is stored where; Save/Revert; scanning; file operations |
| [docs/design-system.md](docs/design-system.md) | colours, type, metrics, icons, widgets |
| [docs/architecture.md](docs/architecture.md) | crates, layering, commands, threading, automation |
| [docs/development.md](docs/development.md) | building, testing, xtask, performance budgets |
| [docs/process.md](docs/process.md) | how we build Dalica with AI agents |
| [docs/control-protocol.md](docs/control-protocol.md) · [docs/mcp.md](docs/mcp.md) | driving Dalica from scripts and AI agents |
| [docs/security.md](docs/security.md) · [SECURITY.md](SECURITY.md) | threat model, hardening rules, reporting vulnerabilities |
| [docs/contributing.md](docs/contributing.md) · [docs/assets.md](docs/assets.md) | contribution rules, asset and licence policy |
| [docs/parity.md](docs/parity.md) | feature-by-feature tracker |
| [docs/roadmap.md](docs/roadmap.md) · [docs/status.md](docs/status.md) | milestones, honest status, current task |

## Building

Not yet. The planned commands are in [`docs/development.md`](docs/development.md):

```sh
cargo run --release -p dalica
cargo xtask ci
```

## Contributing

Humans and agents follow the same rules: read [AGENTS.md](AGENTS.md) first. In short: pure Rust,
clean-room (match behaviour, never copy assets or code), never crash, never lose a photo,
everything is a command, and `cargo xtask ci` green before every commit.

## Credits

Dalica's architecture, design tokens, widgets and agent-driven development process follow
[LightCraft](https://github.com/storytold/lightcraft) and
[PhotoCraft](https://github.com/storytold/photocraft) by the ArtCraft team (MIT OR Apache-2.0).
Dalica may reuse their code; code, docs and rules adapted from them are listed in
[NOTICE](NOTICE).

## License

MIT OR Apache-2.0, at your option (the same licence as PhotoCraft). The `LICENSE-MIT` and
`LICENSE-APACHE` files land with the workspace skeleton (M0).
