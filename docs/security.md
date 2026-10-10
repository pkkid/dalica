Adapted from PhotoCraft (`SECURITY.md`, `book/src/security/*`, revision 59684266; MIT OR Apache-2.0).

# Security

How Dalica treats hostile input, protects the user's photo folders, and limits what automation can
do. Read it before touching a parser (`crates/image`, `crates/meta`, `crates/sidecar`), anything
that writes, moves or deletes files, the directory walker (`crates/scan`), the control channel, the
MCP server, a dependency, or the release workflow. The reporting policy is
[`../SECURITY.md`](../SECURITY.md). The data-safety rules (atomic writes, originals, merge never
replace) are in [`data-model.md`](data-model.md); this document is the adversarial view of the
same code.

> **Status:** nothing is built yet. Every control below is marked **Planned (Mn)** with the
> milestone from [`roadmap.md`](roadmap.md) in which it lands. The commit that lands a control
> changes its mark to **Implemented** and names the code path. Never describe a control as
> implemented before its code and tests are on `main`, and never report a successful build as a
> published release.

## 1. Overview

Dalica reads files it did not create: photos and videos from cameras, phones, messengers and the
web; sidecars written by other Dalica installs on other machines; sidecars and backup folders
written by the legacy organizer; folders on network shares, synced cloud folders and removable
media. It writes into the user's own photo folders. It also exposes a local control channel and an
MCP server that let AI agents drive it with the user's file-system permissions. Security is
therefore part of parser, library, file I/O, automation, dependency and release design, not a layer
added afterwards.

Principles:

1. **Everything from outside is hostile until validated:** file bytes, file and folder names,
   metadata, sidecar contents, JSON lines, MCP arguments and agent decisions.
2. **Fail closed without crashing.** A parser or command returns the crate's error type; it never
   panics, loops forever or allocates without bound (`AGENTS.md` §3).
3. **A hostile input may cost a thumbnail, never an original.** The data-safety rules of
   `AGENTS.md` §2 apply to attackers as well as to bugs.
4. **Authority is explicit and narrow.** Automation addresses photos by library id, not by
   arbitrary path; file-system paths from automation must lie inside roots granted at launch.
5. **Every fixed security bug becomes a permanent regression test** that failed before the fix.

### 1.1 Planned posture

| Area | Planned protections | Lands | Known limits (by design) |
|---|---|---|---|
| Image decoding | dimension, pixel and allocation limits charged to the memory budget; scaled decode; per-file panic guard; fuzz targets | M1 | decoders from dependencies are trusted to honour the limits we pass; we fuzz our wrappers to check |
| Metadata (EXIF/XMP/IPTC/ICC) | IFD count/depth/entry limits, cycle detection, XML depth/node limits, no DTDs or entities, segment reassembly limits; merge-preserving writer; fuzz targets | M1 (read), M10 (write-back) | |
| Folder sidecar `.dalica.json` | size/depth/entry limits, single-component names only, escaped values, unknown data preserved; fuzz + property tests | M1 | a sidecar is as private as its folder |
| Legacy sidecars and backups | read-only fallback when a folder has no `.dalica.json`, same limits, no path from them is ever opened | M33 | |
| Library index, journal, settings | checksummed records, size and depth limits, `.corrupt-<time>` quarantine | M1 | |
| Directory walks | depth 32, 1,000,000 entries, symlink-loop detection, off the UI thread | M1 | |
| File writes | atomic replace with identity check, no writes through symlinks, OS trash only | M1 (sidecars), M10 (metadata), M15 (Save), M19 (file operations) | network file systems weaken atomicity (§5.9) |
| Control channel | loopback only, 256-bit token from a `0600` file, JSON-request-only lines, request/reply/connection/batch limits | M2 | no per-client capabilities, no encryption |
| MCP server | stdio by default, roots, read-only mode, tool annotations, same limits | M21 | an authenticated agent can still trash or caption any photo in the library unless read-only |
| Network features | user-initiated only, TLS, size limits, pinned model hashes | M29, M31 | TLS stack still to be decided (§8.2) |
| Generated output | escaped JSON, sanitized file names | M16 (export), M32 (collage) | |
| Dependencies | pure Rust, banned crates, licence allow-list, advisory check | M0 | scanner results do not replace review |
| Releases | draft releases, `SHA256SUMS.txt`, optional signing secrets, least-privilege jobs | M11 | signing may be absent; consumers must check |

The table is a plan, not a guarantee about any build. Verify source and release evidence before
relying on it.

### 1.2 Priorities

1. M0–M2: the control channel ships with authentication, line validation and limits from its first
   commit; there is no unauthenticated phase. The never-crash lints, `panic_hunt` (M1) and the
   dependency check (§8) land with the workspace.
2. M1: bounded decoders and walkers, the sidecar parser with its fuzz target, and one shared
   atomic-write primitive with the path rules of §5.
3. M10 and M15: metadata write-back with merge fuzzing; Save with originals backup.
4. M21: MCP roots, read-only mode, and server proof for the bridge (§6.3).
5. M29–M37: network responses, collage output, legacy sidecar reading, release signing and SBOMs.

## 2. Threat model

This model covers the desktop app (`dalica`), the headless CLI (`dalica-cli`), the MCP server, the
file formats Dalica reads and writes, and the build and release pipeline. It assumes files, names,
paths, JSON, MCP messages, agent decisions and embedded metadata may be malicious.

### 2.1 Assets

- **integrity of the user's original photos and videos** (highest: they are often irreplaceable);
- integrity of user data: stars, captions, tags, albums, people, places and edit recipes in
  sidecars and file metadata;
- confidentiality of private photos, captions, people names, face regions and locations, and of
  unrelated local files;
- face data (descriptors and assignments), which must never leave the machine;
- availability of Dalica and of the host (memory, disk, CPU);
- the control token;
- integrity and provenance of release artifacts and dependencies.

### 2.2 Threat actors

- the author of a malicious photo or video (downloaded, received in a message, copied from a USB
  stick or a camera card);
- another person or machine with write access to a shared folder: a network share, a synced cloud
  folder, a family NAS. They can plant crafted `.dalica.json` files, crafted legacy sidecars,
  symlinks, junctions and huge directory trees, and can swap files while Dalica writes;
- a malicious removable device (camera, card, phone over MTP/PTP) that presents hostile names,
  counts, sizes or contents;
- a local process connecting to the control port, or squatting it before Dalica starts;
- a web page in the user's browser sending a cross-origin request to `127.0.0.1:<port>`;
- a malicious, compromised, over-eager or prompt-injected AI agent using MCP or the control
  channel; captions, tags and file names it reads may themselves carry injected instructions;
- a network peer for user-initiated features (map tiles, geocoding, model downloads): a hostile
  server or a man in the middle;
- a compromised dependency, CI action, build tool or signing environment.

### 2.3 Entry points

```text
untrusted files and bytes
  JPEG / PNG / TIFF / WebP / GIF / BMP / HEIC / AVIF / JPEG XL / PSD composite / RAW previews
  EXIF + maker notes / XMP (incl. extended XMP) / IPTC / ICC profiles / embedded thumbnails
  .dalica.json sidecars / .dalica-originals/.dalica.json / legacy sidecars and backup folders
  folders.json / albums.json / people.json / settings.json / index journal + snapshot
  video containers (M34) / face models (M29) / map tiles and geocoder replies (M31)

untrusted names
  file and folder names from disks, shares and devices / names inside sidecars / paths in metadata

untrusted control data
  CLI arguments / JSON-lines control requests / MCP requests / agent commands / snapshot scripts

untrusted build inputs
  crates / git dependencies on the Craft apps / CI actions / release secrets / packaging tools
```

### 2.4 Primary threat scenarios

**Hostile photo.** A small file declares 100,000 × 100,000 pixels, nests IFDs in a cycle, carries
an XMP packet with a billion-laughs entity, reassembles extended XMP or ICC chunks into gigabytes,
or truncates every structure. Desired outcome: rejected or decoded at reduced scale before
unreasonable allocation or work, a placeholder tile, an error in the log, the scan continues, and a
minimized regression test.

**Hostile sidecar from another machine.** A `.dalica.json` has a photo key `"../../.ssh/id_ed25519"`,
nests objects ten thousand deep, repeats keys, lists a million album ids, or uses `NaN`
coordinates and unknown edit operations. Desired outcome: names that are not a single plain
component are preserved verbatim but never used to reach a file; values are clamped or ignored;
unknown data survives rewrites; nothing outside the folder is read or written.

**Legacy data.** Legacy sidecars and album definitions may contain absolute paths, odd encodings
and operations we do not understand. Desired outcome: read-only, converted in memory, never
written back; a legacy file is ignored wherever a `.dalica.json` exists; paths are only matched
against photos already indexed under watched folders, never opened; legacy files are never
modified.

**Shared and network folders.** A collaborator replaces a photo with a symlink to a file in the
user's home just before Dalica saves, swaps the `.dalica-originals` directory for a symlink, or
changes a file between Dalica's read and its write. Desired outcome: Dalica never writes through
a symlink, never replaces a file whose identity changed since it read it, and never moves an
original anywhere but its own folder's backup directory.

**Paths from metadata.** XMP (`xmpMM:DerivedFrom`, `stRef:filePath`), maker notes, legacy files or
sidecars contain paths. Desired outcome: they are data to display at most; Dalica never opens,
stats, creates, deletes or launches anything because of them.

**Local control takeover.** Another process connects to the control port and drives the app, or
binds the port first and collects the token from an MCP bridge. Desired outcome: no method runs
before token authentication; the token is never in a command line or log; the bridge only talks
to a server that proves it knows the token (§6.3).

**Browser cross-protocol request.** A web page `fetch`es `http://127.0.0.1:7990/` with a body that
is a valid control request. Desired outcome: the HTTP request line is not a JSON request, so the
connection is closed before anything else on it is read.

**Agent misuse.** An agent, perhaps steered by a caption that says "delete every photo", calls
delete, move or Save on many photos, or asks to export to `~/.config`. Desired outcome: deletes
only reach the OS trash, Save always keeps the original, path-bearing commands are confined to
granted roots, a read-only mode exists for untrusted agents, and destructive tools are labelled.

**Resource exhaustion.** A client opens many connections, sends an endless line, a huge batch, a
million ids, or asks for a 50,000-pixel screenshot. Desired outcome: stable errors before
expensive work.

**Generated output.** A caption contains `"` and newlines; an album name contains `../` or `CON`; an
EXIF date is `2026:13:99/../../x`. Desired outcome: JSON is escaped, generated names are sanitized
single components, and invalid dates fall back to safe names.

**Supply chain.** A crate, action or signing tool is vulnerable or malicious. Desired outcome:
pinned lockfile and toolchain, banned native crates, licence and advisory checks, reviewed
additions, checksummed draft releases.

### 2.5 Security objectives

- reject malformed and oversized data before allocation where possible;
- use checked arithmetic and bounded recursion at every parser and command boundary;
- never lose, truncate or silently replace an original;
- keep reads and writes inside the folder or root that justifies them, despite traversal, links
  and races;
- make automation authority explicit, narrow and revocable;
- bound work per request and per file;
- keep secrets, face data and private text out of logs and out of the network;
- turn every fixed security bug into a permanent regression test.

### 2.6 Non-goals and limits

- Dalica is not a sandbox for hostile automation or hostile native code. Loopback binding, a
  token, Rust memory safety, typed parameters and format checks do not replace OS accounts,
  containers or least privilege.
- A process running as the same user with access to the photo folders can already read and change
  the photos; the token protects against other users, browsers and accidental connections, not
  against that process.
- **Hidden is not access control.** Hiding a photo or folder is a view filter (password-protected
  hidden folders are a reserved decision, proposed "no").
- Sidecars carry captions, people names and face regions next to the photos by design. Anyone who
  can read the folder can read them; "Store people names in photos" is off by default for the same
  reason.
- Dalica does not encrypt its library, cache or sidecars.
- External programs Dalica launches at the user's request (file manager, mail client, external
  editor, browser) are outside its control.

## 3. Trust boundaries

The central boundary is crossed when untrusted bytes, names or requests become library state or a
file-system effect.

```text
UNTRUSTED
  photo / video / metadata bytes          (image, meta)
  .dalica.json, legacy sidecars, JSON files (sidecar, library)
  file and folder names, device contents  (scan)
  CLI arguments, control lines, MCP calls (apps, ui control, mcp)
  network replies (M29)
             |
             v
SECURITY BOUNDARY
  format parsers and decode budgets
  name validation and the atomic-write primitive
  directory walker bounds and link policy
  control/MCP transports: auth, line validation, limits
  command registry: enabled checks, parameter validation, automation policy, root checks
             |
             v
TRUSTED INTERNAL STATE
  Library model and ops, Session and undo stack
  edit recipes, thumbnail jobs
  user-authorized file-system effects
```

"Trusted internal state" means code may rely on validated invariants (ranges, single-component
names, finite numbers). It does not mean the data is harmless: captions, tags, people names, file
names and other text that came from a file stay **tainted** for logs, UI, generated file names,
sidecar and XMP writing, MCP results and external process arguments (§4.4).

### Boundary ownership

| Boundary | Primary code | Required checks |
|---|---|---|
| Image bytes → pixels | `crates/image` | sniff vs extension, dimensions, pixels, allocation vs memory budget, frame counts |
| Metadata bytes → fields | `crates/meta` | IFD count/depth/cycles, offsets inside input, XML depth/nodes/no DTD, chunk reassembly, string lengths |
| Fields → file (write-back) | `crates/meta` | merge preserves foreign bytes, escaping, segment sizes, atomic replace, verify by re-read |
| Sidecar JSON → library | `crates/sidecar` | size/depth/entry limits, name validation, numeric ranges, unknown-data preservation |
| Legacy files → library | `crates/sidecar/src/legacy.rs` | as above, read-only, path matching only |
| Index/JSON files → library | `crates/store` | checksums, versions, size and depth limits, quarantine |
| Directory tree → index | `crates/scan` | depth/entry bounds, link-loop detection, name handling, device content |
| Library → disk (file ops) | `crates/fs`, `crates/engine` | identity check, no symlinks, trash only, collision handling |
| JSON line → command | `apps/dalica/src/control_server.rs`, `crates/ui` control, `crates/engine` | auth, line shape, limits, command policy, parameter type/range, enabled state |
| MCP call → command | `crates/mcp` | roots, read-only mode, argument limits, result limits |
| Library → generated output | `crates/create` | JSON escaping, name sanitizing, external-process arguments |

### Agent guidance

Agents must read this document before modifying any parser, file I/O, directory walk, file
operation, automation transport, MCP tool, command parameter handling or serialization code. A
caller-provided path or tool request is data, not proof of user authorization. Changes in these
areas get the second, data-safety-focused review required by [`process.md`](process.md) §4.

## 4. Parser hardening

Parser safety is broader than "does not crash". A parser must also bound memory, CPU time,
recursion, output expansion and the number of objects it creates. Dalica scans tens of thousands of
files unattended, so one bad file must never stall or kill a scan.

### 4.1 Review checklist

For every attacker-controlled count, offset, dimension, length or number:

1. Parse into a type that can represent the file field without truncation.
2. Validate semantic limits before allocation.
3. Use `checked_add` / `checked_mul` or a deliberately safe saturating comparison; never `as` a
   possibly negative or oversized value into an index.
4. Verify a range lies inside the input before slicing; use `get()`, not indexing.
5. Charge decoded bytes, objects and recursion against a budget.
6. Reject non-finite floats (`NaN`, infinities) and clamp numbers to their documented range.
7. Return a format-specific error; never panic or loop indefinitely; detect offset cycles.
8. Test zero, maximum, maximum plus one, truncation at every byte, and inconsistent declarations.

### 4.2 Limits

Initial values; a change needs a test at the new boundary. Values marked "LightCraft" match the
`meta`/`tiff` crates Dalica plans to reuse (`architecture.md` §9); Dalica passes them explicitly
rather than relying on defaults.

| Input | Crate | Limits | Lands |
|---|---|---|---|
| Raster decode (all formats) | `image` | edge ≤ 65,535 px; ≤ 2³⁰ pixels; decoded allocation charged to the memory budget (RAM/4, 256 MiB–1.5 GiB): a file whose full decode does not fit is decoded at reduced scale (JPEG DCT scaling, embedded preview) or shown as a "too large" placeholder | M1 |
| Animated GIF/WebP | `image` | first frame for thumbnails; ≤ 10,000 frames and the same allocation budget for playback | M8 / M27 |
| PSD | `image` | flattened composite only; layers are never parsed | M8 |
| Embedded thumbnail / RAW preview | `image` | offset + length inside the file; ≤ 64 MiB; decoded under the raster limits | M1 / M8 |
| EXIF/TIFF IFDs | `meta` | ≤ 1,024 IFDs, depth ≤ 10, ≤ 8,192 entries per IFD, visited-offset cycle detection (LightCraft); maker notes ≤ 64 IFDs, depth ≤ 3; strings ≤ 64 KiB; zero-denominator rationals are invalid values | M1 |
| XMP | `meta` | packet ≤ 16 MiB after extended-XMP reassembly (matching GUID, chunk offsets inside the declared length); XML depth ≤ 64, ≤ 200,000 nodes (LightCraft); `DOCTYPE`, entity declarations and external references rejected | M1 |
| IPTC (IIM in APP13 / 8BIM) | `meta` | resource and dataset lengths inside the segment; dataset ≤ 64 KiB; ≤ 10,000 datasets | M1 |
| ICC profile | `image` | APP2 chunk sequence validated; ≤ 16 MiB; tag count ≤ 1,000; LUT sizes bounded by the colour library | M8 |
| `.dalica.json` | `sidecar` | file ≤ 64 MiB; nesting depth ≤ 16; string ≤ 1 MiB; ≤ 1,000,000 photo entries; ≤ 1,000 keys per entry; ≤ 10,000 operations per recipe; ≤ 1,000 regions per operation; list entries ≤ 100,000 | M1 |
| Legacy sidecars | `sidecar` | file ≤ 64 MiB; line ≤ 1 MiB; ≤ 1,000,000 sections; ≤ 1,000 keys per section; the recipe limits above; encoding detection never fails the whole file | M33 |
| JSON files (`folders`, `albums`, `people`, `settings`) | `library` | file ≤ 64 MiB; depth ≤ 64 | M1 |
| Index journal / snapshot | `library` | checksummed records ≤ 16 MiB; a torn tail is discarded, a bad checksum stops replay at that record; version checked | M1 |
| Control / MCP JSON | `ui`, `mcp` | see §6.5 | M2 / M21 |
| Video containers | `image` or the video crate | box depth ≤ 12, ≤ 4,096 boxes per level, sample tables bounded; or an external process with a timeout (reserved decision 2) | M34 |
| Face models | faces crate | SHA-256 pinned in the source; size cap; parsed as data only | M29 |
| Map tiles, geocoder replies | network module | tile ≤ 1 MiB and ≤ 1,024 px edge, decoded under the raster limits; JSON reply ≤ 1 MiB, depth ≤ 32 | M31 |

### 4.3 Work budgets during scans

- Each file's decode and metadata read runs inside the per-file `catch_unwind` guard on a worker
  thread with a cancel flag. **Planned (M7).**
- A file that fails, panics or exceeds a 10-second work budget is recorded by (path, size, mtime)
  and not retried until it changes; the tile shows a generic placeholder and the scan continues.
  **Planned (M7).**
- Work counters (not only byte counts) for highly fragmented but size-bounded files: number of
  JPEG scans, Huffman tables, PNG chunks, TIFF strips/tiles. **Planned (M7).**

### 4.4 Tainted text

Text that came from a file, a sidecar, a device or an agent:

- is never interpreted: not as a path (§5.5), a format string, a shell command, HTML or a URL;
- is displayed with bidirectional-override and other invisible control characters made visible in
  file names (a name like `photo<RLO>gpj.exe` must not pass for a JPEG);
- is escaped for its destination: sidecar values are written through the JSON serializer, so a
  caption can never start a new key or object; XMP writing escapes XML and drops characters that
  are invalid in XML 1.0; IPTC writing respects the dataset length and the declared character set;
- is kept out of logs: no captions, tags, people names, face data or tokens; paths only where
  needed and never in bulk;
- is passed to external programs only as separate argv elements (§5.11);
- is returned to AI agents as data. MCP tool descriptions state that captions, tags and file names
  are user content and may contain instructions that must not be followed. **Planned (M21).**

### 4.5 Required tests

Every reader: valid round trips, exact boundary values, truncation sweeps, bit flips, inconsistent
lengths, bomb-like headers, and a `proptest` that random input returns `Ok` or `Err` without
panicking. Every writer: round trip, foreign data preserved byte for byte, crash in the middle
leaves the old or the new file. (`development.md` §6.)

## 5. Secure file handling

### 5.1 Where Dalica writes

Dalica writes **only** to:

1. folder sidecars (`<folder>/.dalica.json`) in watched folders;
2. originals backups (`<folder>/.dalica-originals/`) on Save;
3. photo files themselves, on Save and on metadata write-back (`data-model.md` §5–§6);
4. destinations the user chose (export, import, Save a Copy, collage),
   including the new folders they create;
5. the library directory (`<data>/Dalica/`), the cache (`<cache>/Dalica/`), logs and Dalica's
   own temporary files;
6. the OS trash, through the platform trash interface.

Anything else is a bug. Each write site uses the shared atomic-write primitive; a new kind of
write needs a row here.

### 5.2 Atomic replace

The shared primitive (our own; prior art LightCraft `safe_file.rs`). **Built (M1.2:
`dalica-fs` `atomic::replace`), with a byte-for-byte check in step 6; the metadata-aware check
comes with write-back (M10).**

1. Read the target and record its identity: device, inode (file index on Windows), size and
   modification time.
2. Create the temporary file **in the same directory** with exclusive creation (`create_new`), so
   the final rename stays on one volume (§5.8 for its name).
3. Write, `fsync` the file, copy the target's permission bits.
4. Re-check the target's identity (§5.3). If it changed, delete the temporary file and report a
   conflict.
5. Rename over the target; on Unix `fsync` the directory.
6. Verify by re-reading (for metadata write-back: parse it and compare the fields we wrote and a
   hash of the pixel data segments).

Hard links: the rename gives the target a new inode, so other links to the old file keep the old
contents. That is intentional (Dalica never modifies a file through another name).

### 5.3 Never overwrite what you did not just read

A write that replaces a file proceeds only if the file's identity is unchanged since Dalica read
the content it is replacing. This covers external edits, sync clients and a collaborator swapping
the file on a share. Save also checks the original's identity immediately before moving it into
`.dalica-originals/`, and refuses if a backup with that name already exists but does not match the
recorded original (the first original is the one kept). **Built for every `fs::replace` (M1.2:
`Expect::Unchanged`); Save's checks are planned (M15).**

### 5.4 Names

**Names read from sidecars.** A photo key under `"photos"`, an album or person id, an
`"order"` entry or a name in `.dalica-originals/.dalica.json` is used to reach a file only if it is a
single plain path component: not empty, not `.` or `..`, no `/`, `\` or NUL, no drive or stream
prefix (`C:`, `name:stream`), not a Windows reserved device name (`CON`, `NUL`, `COM1`, …). Anything
else is preserved verbatim on rewrite, never resolved, and reported once. Ids (`AlbumId`,
`PersonId`) must match their documented shape. **Planned (M1).**

**Names Dalica generates** (export folders from album names, import folders from capture dates or
typed text, Save a Copy and collision suffixes, collage files) go through one sanitizer: remove path separators, `: * ? " < > |`, control and
bidirectional characters; strip leading `.` and `-` and trailing dots and spaces; replace reserved
device names; limit to 255 bytes on a character boundary; never empty (fallback such as
`Export 2026-06-04`). Collisions are resolved with `-001`, `-002`, … using exclusive creation in a
loop, not "check then create". Dates from EXIF are parsed and re-formatted, never copied as text.
**Planned (M16, M23, M32).**

**Names from the file system.** File names that are not valid UTF-8 are indexed and shown with a
lossy display form, but the lossy form is never written into a sidecar and never used to open a
file (two different names could collapse to the same lossy string and attach one photo's data to
another). Matching sidecar entries to files is exact first; case-insensitive or
Unicode-normalization-insensitive matching (macOS, Windows, SMB) applies only when it is
unambiguous. **Planned (M1).**

### 5.5 Paths from metadata are never followed

Paths found in XMP (`xmpMM:DerivedFrom`, `stRef:filePath`, …), maker notes, IPTC, sidecars, legacy
sidecars and legacy album definitions are data. Dalica never opens, stats, creates, deletes, moves
or launches anything because of them. Legacy album members are matched only against photos already
indexed beneath watched folders, by comparing paths in memory. ICC profiles are used only when
embedded, never loaded by a referenced name. **Planned (M1, M33).**

### 5.6 Symlinks, junctions and hard links

- Directory walks follow directory symlinks and junctions with (device, inode) loop detection
  inside the depth and entry bounds of `data-model.md` §7. **Built (M1.3: `dalica-fs` `walk`;
  folder links are followed after the real tree, so a real folder always wins over a link to it).**
- **Writes never go through a link.** Sidecars, photos on Save or metadata write-back, the
  `.dalica-originals` directory and its contents are opened without following a final symlink
  (`O_NOFOLLOW`-style). If the target is a link, or a folder reached through a link resolves
  outside every watched root, Dalica does not write there: the folder is treated as read-only and
  shows a `caution` notice. The exact UI and how user data for such folders is kept are settled in
  M1 (`data-model.md` §7). **Planned (M1).**
- Moves and renames inside Dalica operate on the link, not its target, and never follow a link
  out of the source or destination folder. **Planned (M19).**
- Where the platform allows, the write path holds a directory handle and works relative to it,
  so a component swapped for a link after validation cannot redirect the write. Canonicalization
  alone is insufficient because links can change after the check. **Planned (M7, through `rustix`;
  until then the write path compares the file's identity before and after opening it).**

### 5.7 Trash, never delete

- Deleting a photo, video or folder moves it to the **OS trash** after confirmation
  (`data-model.md` §8). Dalica has no permanent-delete path for library content, in the UI or over
  automation. **Planned (M19).**
- If the platform trash is unavailable for a volume (some network shares and removable media), the
  delete fails with an explanation that points to the file manager. It never falls back to a
  permanent delete. **Planned (M19).**
- "Remove from Dalica", "Rebuild library" and cache pruning never touch photos. Caches list and
  delete only files of their own naming shape. Stale sidecar entries are pruned, not files.
- **Camera and card import** ("Delete copied files from the card"): cards usually have no trash,
  so this is the only permanent deletion Dalica performs. They
  happen only after every copied file has been verified by hash at its destination, only for files
  the import listed, after an explicit confirmation, and they are refused over automation.
  **Planned (M23).**
- Moving across volumes is copy + verify (hash) + delete-to-trash of the source, never delete
  before verify. **Planned (M19).**

### 5.8 Temporary files

- Atomic-replace temporaries live next to their target, named `.<name>.dalica-tmp-<random>`,
  created exclusively. Dalica removes only stale files of exactly this shape (older than 24 hours)
  when it next scans the folder. **Names built (M1.2: `atomic::is_temp_name`); the stale cleanup
  is planned with scanning (M1).**
- Other scratch files (render and export scratch, decoded previews spilled to disk) go to
  `<cache>/Dalica/tmp/`, created `0700` on Unix, with random names, never to a predictable name in
  a shared system temp directory. **Planned (M13).**
- A crash leaves at most a stray temporary file, never a truncated target.

### 5.9 Network shares, synced folders and removable media

- Network file systems may not make rename atomic, may ignore `fsync`, and may not support
  rename-over. Dalica still never truncates a target in place: if rename-over fails it renames the
  target to a temporary backup name, renames the new file into place, verifies, and only then
  removes the backup. **Planned (M1).**
- The library lock file lives in the local library directory, not on the share, because file
  locks on network shares are unreliable.
- A volume that goes offline during a write leaves the old file; pending changes stay queued in the
  index (`data-model.md` §7).
- Slow or hung shares never block the UI thread; file-system answers used while painting come
  from worker-filled caches (`architecture.md` §5).
- Sync clients create conflict copies (for example `.dalica (conflict).json` style names). Dalica
  does not read or merge them automatically; it may report them. **Planned (M1).**
- Devices (cameras, cards, phones) are read as untrusted trees: the walker bounds apply, names are
  validated, and sizes reported by the device are not trusted for allocation. **Planned (M23).**

### 5.10 Private data at rest

- On Unix, the library directory, the cache directory, the control-token directory and the logs
  are created `0700` (files `0600`); on Windows they live under the user's profile. **Planned
  (M0–M2).**
- The thumbnail cache holds images of every indexed photo, including hidden ones; it is private to
  the user and deletable at any time.
- Face descriptors live only in the local index; they are never written to sidecars, file
  metadata, logs or the network.
- Logs contain no file contents, captions, people names, face data, tokens or request bodies, and
  minimize full paths.

### 5.11 External programs

- Programs are started with an argument vector, never through a shell; paths come after `--`
  where the program supports it, and a path starting with `-` is passed as `./-name`.
- Only user actions in the UI launch external programs (Open With, Open in External Editor, Locate
  on Disk). These commands are refused over automation (§6.6). **Planned (M36).**
- URLs Dalica opens are fixed `https://` addresses from its own configuration (map attribution,
  help), never URLs found in metadata.
- An optional external video process (reserved decision 2) runs with a timeout, without a shell,
  with only the input path and fixed arguments; its output is parsed with the limits of §4.2.

### 5.12 Tests

`..`, absolute paths, mixed separators, device and stream names, case and normalization collisions,
non-UTF-8 names, symlinks and junctions at every path component, a file replaced between read and
write, a missing trash, a full disk, an unplugged volume mid-write, and a crash at every step of the
atomic-write sequence (fault injection). These extend the data-model regression list
(`data-model.md` §10).

## 6. Automation security

Automation is a privilege boundary: requests can read and change library data, move files to the
trash, rewrite photos on Save, write exports and screenshots, inject UI input and change
preferences, all with the user's file-system permissions.

### 6.1 Transports

| Transport | Started by | Authentication | Lands |
|---|---|---|---|
| Desktop control channel, JSON lines over TCP | `dalica --control <port>` (default 7990; parallel agents 18000–19999) | token (§6.3) | M2 |
| Headless script on stdio | `dalica-cli run`, `dalica-cli snapshot --script` | inherited from the process pipe | M1, M2 |
| MCP over stdio, headless engine | `dalica-cli mcp [--demo]` | inherited from the process pipe | M21 |
| MCP bridge to the running app | `dalica-cli mcp --connect 127.0.0.1:<port>` | token (§6.3) | M21 |

Prefer stdio. Start the TCP control channel only when needed.

### 6.2 Loopback only

The control server binds `127.0.0.1:<port>` only. It never binds `0.0.0.0`, `::` or a LAN address,
and refuses any configuration that would. If the port is taken it fails with a clear error instead
of choosing another port. `--connect` accepts only loopback addresses. The protocol is unencrypted:
never tunnel or proxy it to another host. **Planned (M2).**

### 6.3 Authentication and the token file

- The token is 256 bits from the OS random source, written as 64 hexadecimal characters.
- `dalica --control <port>` reads the token from `--control-token-file <path>`
  (`DALICA_CONTROL_TOKEN_FILE`), default `<data>/Dalica/control/<port>.token`. If the file does
  not exist, Dalica creates it with a fresh token: on Unix mode `0600` inside a `0700` directory;
  on Windows with an ACL that grants only the current user. An existing file is reused only if it
  is a regular file (not a link) owned by the current user and, on Unix, not readable or writable by
  group or others; otherwise the control channel does not start and says why.
- There is no option to pass the token on the command line, where other users can see process
  arguments.
- Clients (`dalica-cli mcp --connect`, `dalica-cli run --connect`) read the same default file for
  the port, or the file named by `--control-token-file`.
- The first line on every connection must be
  `{"id":"auth","method":"auth","params":{"token":"<64 hex>"}}`. No other method is dispatched
  before it succeeds. A wrong token, any other first method, or no `auth` within 10 seconds gets
  one error reply and the connection is closed. The comparison is constant-time. (Examples
  elsewhere in the docs omit the `auth` line for brevity; scripts and `nc` sessions send it first.)
- Tokens are never logged, never included in error messages and never written to `ui.inspect`
  or MCP results.
- **Server proof.** A bearer token alone lets a process that binds the port before Dalica collect
  the token from a connecting bridge. Before the MCP bridge ships, the handshake becomes
  challenge-response: the server greets with a nonce, the client answers with an HMAC of both
  nonces under the token, and the server proves knowledge of the token the same way before the
  client sends anything else. **Planned (M21).**

Bearer-token authentication with the token file: **Planned (M2).**

### 6.4 Only JSON request lines are read

Every line must be a UTF-8 JSON **object** with a string `method` (`id` and `params` optional;
blank lines are skipped). Anything else (text that is not JSON, a JSON array or number, an object
without `method`, invalid UTF-8, or a line over the size limit) gets one error reply
`{"ok":false,"error":"… closing the connection"}` and the server closes the connection; nothing
sent after it on that connection runs. Junk never reaches the UI thread.

This blocks request smuggling from a browser: a web page's cross-origin `fetch` or form post to
`127.0.0.1:<port>` begins with an HTTP request line (`POST / HTTP/1.1`), which is rejected before
its body is read, and it cannot know the token anyway. The server never answers with anything that
looks like HTTP. **Planned (M2).**

### 6.5 Limits

Applied before command dispatch or file effects, with stable error messages.

| Limit | Value | Lands |
|---|---|---|
| Encoded request line (TCP and Dalica's own stdio JSON lines) | 1 MiB | M2 |
| JSON nesting depth in a request | 64 | M2 |
| Encoded reply line | 8 MiB, including the newline | M2 |
| Active TCP connections per listener | 16; further connections get one error line and are closed | M2 |
| Socket read/write timeout | 30 s; clients reconnect | M2 |
| Wait for the UI thread to answer | 60 s; a request still queued then is rejected before dispatch (a timeout does not cancel work already started) | M2 |
| Time to authenticate | 10 s | M2 |
| Ids per command (`ids` arrays) | 100,000 | M2 |
| `ui.screenshot`, `dalica-cli snapshot`, `ui.resize` | ≥ 1 px and ≤ 16,384 px per edge, ≤ 64 million pixels, finite positive scale; checked before layout or rasterization | M1, M2 |
| Batch requests (control or MCP) | ≤ 256 steps; an aggregate reply budget stops later steps when exhausted | M21 |
| Query results (photo lists) | paged, ≤ 10,000 items per page | M21 |
| Rendered previews returned to agents | ≤ 2,048 px edge by default; encoded image ≤ 5 MiB | M21 |
| MCP tool arguments / results | arguments ≤ 1 MiB; encoded results ≤ 8 MiB | M21 |

A reply can be rejected after a command has already changed state; such errors say that the
operation may have completed, and earlier batch steps are not rolled back. Inspect state before
retrying. A bounded reply does not imply a bounded command cost.

### 6.6 Command policy and file-system authority

- **Ids, not paths.** Commands address photos, folders, albums and people by library id. An id can
  only reach files already indexed under a watched folder, so most commands need no path
  authority at all.
- **Path-bearing commands** (add a watched folder, export destination, import source and
  destination, Save a Copy to another folder, collage destination, `ui.screenshot {path}`, render-to-file) accept a path from automation only if it resolves
  beneath a root granted at launch:
  - `--automation-root <dir>` (repeatable): read and write;
  - `--automation-read-root <dir>` (repeatable): read only;
  - `--demo` grants the demo library's folders;
  - screenshot and render outputs (`.png`, `.jpg`) may also go to the system temporary directory,
    replacing only a regular file, never a link;
  - with no matching root the command fails closed.
- **Path rules.** Reject NUL, `..` components (before resolving), device names, `\\?\` and `\\.\`
  prefixes, alternate data streams and drive-relative paths. Resolve the parent, then compare
  path **components** against the root (a string prefix would accept `/photos-evil` for
  `/photos`). The final component must not be a link. A new file's parent must already exist under
  the root; automation does not create arbitrary directory trees (export creates its single,
  sanitized output folder).
- **Refused over automation:** commands that open a native file dialog (supply the path parameter
  instead), launch external programs (§5.11), permanently delete card contents (§5.7), change the
  watched-folder list or other file-bearing preferences except through the root-checked commands
  above, or download anything (models, tiles). Individual safe preferences remain settable.
- **Same policy for every route.** Synthetic UI input (`ui.clickWidget`, `ui.key`, `ui.type`,
  `ui.menu.invoke`) goes through the same policy as `engine.execute`, and so does every command a
  command runs on its own behalf (batch edit, actions, import steps), so an allowed command cannot
  reach a refused one.
- **Destructive commands stay safe by construction:** delete goes to the trash, Save keeps the
  original in `.dalica-originals/`, metadata write-back merges. Automation has no way around these.
- **Read-only mode.** `--automation-read-only` (control channel) and `dalica-cli mcp --read-only`
  expose only commands that do not change files, sidecars or the library. Use it for untrusted
  agents. **Planned (M21).**

Command policy and roots: **Planned (M2)** for `ui.screenshot`; **Planned (M16–M23)** for the
other path-bearing commands as they land.

### 6.7 MCP specifics

- Headless `dalica-cli mcp` grants file-system authority with `--root <dir>` and `--read-root
  <dir>` (same semantics as §6.6). If the MCP client advertises roots, the effective authority is
  the **intersection** of the client's roots and the launch roots; a client can narrow authority,
  never widen it. Roots are re-evaluated when the client reports a change.
- In bridge mode (`--connect`), roots and read-only mode are configured on the `dalica` process;
  the bridge cannot widen them.
- Tools carry MCP annotations: read-only tools are marked read-only; delete, move, rename, Save,
  Revert and metadata-writing tools are marked destructive so clients can ask the user first.
- Tool descriptions and results treat captions, tags, file names and people names as untrusted
  user content (§4.4). Results never include tokens, face descriptors or absolute paths outside
  the granted roots.
- The bridge bounds incoming replies, drops the connection after a transport failure or timeout,
  reports that the operation may have completed, and never resends it automatically.

All of §6.7: **Planned (M21).**

### 6.8 Control status

| Control | Status |
|---|---|
| Loopback-only TCP binding | Planned (M2) |
| Token authentication from a `0600` token file | Planned (M2) |
| JSON-request-only lines, close on junk | Planned (M2) |
| Request, reply, depth, connection, timeout limits | Planned (M2) |
| Screenshot / snapshot dimension limits | Planned (M1, M2) |
| Command error contract and `panic_hunt` | Planned (M1) |
| Root-checked path parameters | Planned (M2 screenshots; M16–M23 others) |
| Batch limits, paging, preview limits | Planned (M21) |
| MCP roots, read-only mode, tool annotations | Planned (M21) |
| Server proof in the handshake | Planned (M21) |
| Per-client capabilities, audit events | Proposed, not scheduled |

### 6.9 Known limitations and proposed capability model

- A client with the token receives the whole non-file-system control surface: UI input, command
  execution, preferences, quitting the app, and every library photo by id. Read-only mode is the
  only finer grant planned.
- No encryption, no client identity, no structured audit stream; one thread per accepted
  connection.
- Expensive commands (export, collage, batch edit) are not charged to a per-session budget beyond
  their own cancellation.

A future gateway could turn the authenticated connection into a short-lived, capability-scoped
session (`LibraryRead`, `LibraryWrite`, `FilesRead`, `FilesWrite`, `UiInspect`, `UiControl`,
`PreferencesWrite`, `ApplicationControl`), with audit events recording the method or command id,
outcome, duration and a redacted path handle, never file contents, captions or tokens. Names and
granularity are a proposal.

### 6.10 Deployment guidance

Prefer stdio. Start the control channel only when needed. Grant the narrowest roots. Keep the
token file private and never commit or log it. Use `--demo` or read-only mode for agents you do
not trust, and a separate OS account, VM or container for untrusted agents near a real photo
library. Never point an experimental agent at the only copy of a photo collection.

## 7. Fuzzing

### 7.1 Targets

Each target lives in a `fuzz/` workspace next to its crate, separate from the main workspace
because `cargo-fuzz` uses nightly Rust and sanitizer instrumentation. A target lands in the same
commit as the parser it covers. Where a parser comes from a crates.io dependency, Dalica still fuzzes
its own entry points, because the limits are the ones Dalica passes.

| Crate | Targets | Lands |
|---|---|---|
| `crates/image` | `sniff`, `decode_any`, `decode_jpeg`, `decode_png`, `decode_tiff`, `decode_webp`, `embedded_thumbnail` (EXIF thumbnail and RAW preview extraction), `icc` | M1 (`decode_tiff`, `decode_webp`, `icc`: M8) |
| `crates/meta` | `exif` (IFDs and maker notes), `xmp` (including extended-XMP reassembly), `iptc`, `write_merge` (parse → merge-write → re-parse: no panic, foreign fields unchanged) | M1 (read), M10 (write) |
| `crates/sidecar` | `parse`, `roundtrip` (parse → write → parse is stable, unknown data preserved), `recipe` (edit-operation list), `legacy` | M1 (`legacy`: M33) |
| `crates/store` | `journal` (replay of journal records and snapshots), `json` (folders, albums, people, settings) | M1 |
| `crates/ui` control | `request_line` (line validation, auth state machine, limits) | M2 |
| `crates/mcp` | `tool_args` | M21 |
| video demuxer | `demux` (if a pure-Rust path is chosen) | M34 |

Command parameters are fuzzed by `panic_hunt` (M1), which runs every registered command with
hostile parameters and must stay green.

### 7.2 Running

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
cd crates/sidecar
cargo +nightly fuzz run parse -- -max_total_time=300
```

Reproduce a crash, minimize it (`cargo fuzz tmin`), add a deterministic regression test that fails,
then fix it. A time-limited run with no findings is evidence of that run only, not proof that a
parser is secure.

CI: a manually dispatched fuzz job lands with the first target (M1); a scheduled weekly run of every
target for a fixed time lands by M37. Continuous hosted fuzzing is future work.

### 7.3 What counts as a finding

- panic or abort;
- integer overflow or out-of-bounds access;
- unreasonable allocation or decompression expansion;
- timeout, infinite loop or algorithmic complexity spike;
- recursion or stack exhaustion;
- malformed input accepted into an invalid internal state (non-finite numbers, out-of-range
  coordinates, multi-component names);
- different results between sniffing and explicit-format decoding;
- a round trip that changes or drops foreign metadata or unknown sidecar data;
- a corrupt journal record accepted despite a checksum mismatch.

### 7.4 Corpus policy

Seed corpora are procedural (`dalica-scenes`) or CC0 files from the pinned corpus; never personal
photos, customer data, credentials or an undisclosed weaponized sample. Every confirmed defect
produces a minimized, permanent regression test when disclosure and licensing allow, recording the
parser, the expected error class, the resource budget and the issue or advisory reference after
disclosure. Fuzz corpora and crash artifacts are not committed except as minimized regression
inputs.

## 8. Dependency policy

Cargo dependencies are pinned through `Cargo.lock`, the toolchain through `rust-toolchain.toml`, and
builds use `--locked`. Parser, image, metadata and network dependencies deserve extra review because
they process attacker-controlled data.

### 8.1 Pure Rust

- No C/C++ code is compiled or linked into the product. A `cc`, `cmake`, `bindgen`, `pkg-config` or
  `vcpkg` build dependency in the normal or build graph of `dalica` and `dalica-cli` fails the
  check, unless it is a reviewed exception in `xtask/src/deps.rs` (`C_BUILD_ALLOW`) whose condition
  holds in the resolved features: e.g. `wayland-sys` probes with `pkg-config` only without
  `dlopen`, `wayland-backend` compiles its C log shim only with `log`. The condition is re-checked on
  every run, so the gate bites again if a feature flips. Adding an exception needs maintainer review
  (the first three were confirmed at the M0 review, 2026-10-09).
- Banned in the product: `openssl`, `openssl-sys`, `native-tls`, `ring`, `aws-lc-rs`, `aws-lc-sys`,
  `libz-sys`, `zstd-sys`, and any other crate that builds or statically links C.
- `*-sys` crates are allowed only from an allow-list of binding-only crates that compile no C,
  each with a reason: `windows-sys` (Win32 bindings), `linux-raw-sys` (syscalls), `core-foundation-sys`
  and the `objc2` family (macOS frameworks), `wayland-sys` (loaded at run time), `renderdoc-sys`
  (loaded at run time, debugging only), `js-sys` / `web-sys` (wasm only), `jni-sys` / `ndk-sys`
  (target-gated, unused on desktop). Adding an entry needs maintainer review.
- Dev-only and fuzz-only exceptions (for example `libfuzzer-sys`, which builds libFuzzer) stay out
  of the product graph and are checked as such.
- The check runs on the resolved graph (`cargo metadata` / `cargo tree -e normal,build`) for the
  Linux, Windows and macOS targets, not by grepping `Cargo.lock`, which also lists optional and
  other-target crates.

### 8.2 Network stack

Map tiles, geocoding and model downloads (M29, M31) need TLS. The usual Rust TLS providers (`ring`,
`aws-lc-rs`) contain C or assembly, and LightCraft's lockfile already contains `ring`. Before M29 a
decision is recorded: a pure-Rust crypto provider for `rustls`, an optional clearly separated
network feature, or delegating downloads to the platform. Until then the product contains no TLS
code. It is reserved decision 9 in `roadmap.md`.

Whatever the stack: certificate validation is never disabled, downloaded models are checked against
a SHA-256 pinned in the source, response sizes are capped (§4.2), and requests use a generic
User-Agent with no personal details.

### 8.3 Licences

Allowed for dependencies: `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`,
`BSD-3-Clause`, `ISC`, `Zlib`, `BSL-1.0`, `Unicode-3.0`, `Unicode-DFS-2016`, `CC0-1.0`, `MIT-0`.
`MPL-2.0` only as a reviewed per-crate exception for unmodified dependencies. Never `GPL`, `LGPL`,
`AGPL`, `SSPL` or unlicensed crates (`process.md` §3). Asset licences are a separate check
(`cargo xtask assets`).

### 8.4 Advisories, sources and duplicates

- RustSec advisories fail the check. An exception needs an owner, a rationale, a compensating
  control and a review date. Unmaintained-crate warnings are reviewed, not ignored.
- Sources: crates.io, plus git dependencies on the LightCraft and PhotoCraft repositories pinned to
  a full commit hash (never a branch). Other git sources are refused.
- Duplicate versions warn; duplicates of parser crates are reviewed.

### 8.5 Checks

```sh
cargo deny check            # licences, bans, advisories, sources (deny.toml)
cargo xtask deps            # offline: banned crates and the *-sys allow-list on the product graph
```

`deny.toml` and `cargo xtask deps` land with the workspace. **Planned (M0).** `xtask deps` runs
inside `cargo xtask ci`; `cargo deny check` needs the advisory database from the network, so it
runs as its own CI job. CI tools and GitHub Actions are dependencies too: actions are pinned to a
full commit hash and reviewed.

### 8.6 Adding a dependency

The commit body says why the crate is needed, whether it processes untrusted input, its `unsafe`
and native-code exposure, `build.rs` and proc-macro behaviour (no network access at build time),
supported targets, maintenance status and licence. Craft-app code is never copied (`architecture.md` §9);
any other vendored code keeps a header naming its origin and gets a `NOTICE` row. A scanner result does not
replace reading the code. Community batches follow the checklist in `process.md` §4 (new
dependencies, `build.rs` or CI changes, `unsafe`, hidden Unicode, network access, file writes
outside the library).

## 9. Security testing

- `cargo xtask ci` on every commit (never-crash lints, `#![forbid(unsafe_code)]`, tests, `xtask
  deps`).
- `cargo test -p dalica-engine --test panic_hunt -- --ignored` when commands are added or changed.
- Malformed-input and boundary tests for every reader (§4.5); fault-injection tests for every
  writer (§5.12).
- Control-channel tests: unauthenticated methods refused, wrong token closes, an HTTP request line
  closes the connection, oversized and deeply nested lines rejected, the 17th connection refused,
  screenshot limits enforced, paths outside roots refused, refused commands refused through every
  route (`engine.execute`, `ui.menu.invoke`, `ui.key`, nested commands).
- MCP tests: roots intersection, read-only mode hides and refuses writers, results bounded.
- Workflow acceptance tests (`crates/mcp/tests/workflows.rs`) include a hostile folder: crafted
  sidecars, links, bombs and odd names, checked only through public state and the files on disk.

## 10. Release security

Releases follow the Craft apps' workflow (`process.md` §7). **Planned (M11; signing M37).**

- A `release` branch builds platform packages (AppImage, Flatpak, deb/rpm, MSI, DMG) and creates or
  updates a **draft** release; a maintainer publishes it after checking. Building, signing, drafting,
  publishing and deploying are separate states.
- Workflow permissions default to `contents: read`. Build jobs that read no secrets run without
  the `release` environment, so a branch dry run can run them. Only the signing jobs use the
  `release` environment with its secrets, and only the final release job gets `contents: write`.
- Signing and notarization (macOS, Windows) are conditional: every signing secret is optional, and
  a missing one produces an unsigned artifact and a warning, never a failed build. The job summary
  and release notes state the signature state of every artifact. Consumers verify the signature
  rather than assume one.
- Every release carries `SHA256SUMS.txt` over all artifacts.
- Builds use the pinned toolchain and `--locked`.
- Dalica has no built-in updater; updates come through the release page or the package channel.
- Packages request the narrowest platform permissions that still let users add their own folders
  (decided with the Flatpak manifest at M11).

Future hardening: a machine-readable SBOM per release (for example with `cargo auditable` /
`cargo cyclonedx`), provenance attestations tied to the source commit and workflow, explicit
signature state in release metadata, checksum and signature verification before publication,
documented key rotation and compromise response.

## 11. Vulnerability reporting

The repository-level [`SECURITY.md`](../SECURITY.md) is the authoritative policy: supported versions,
how to reach the maintainers, what to include and what is in scope.

In short: do not publish exploit details, weaponized files, tokens or sensitive crash data before the
maintainers have had a reasonable opportunity to fix the problem. The project does not yet have a
private reporting channel; ask the maintainers for a private transfer method. For a malicious test
file, first send a description, its SHA-256, format, size and how to reproduce, never the file in a
public issue, and never a real personal photo.

When a report is fixed: add the regression test (and a fuzz seed if a parser was involved), note
the fix in the release notes, and update the affected status marks in this document.
