Adapted from PhotoCraft (`SECURITY.md`, `book/src/security/*`, revision 59684266; MIT OR Apache-2.0).

# Security

How Dalica treats hostile input, protects the user's photo folders, and limits what automation can
do. Read it before touching a parser (`crates/image`, `crates/meta`, `crates/sidecar`), anything
that writes, moves or deletes files, the directory walker (`crates/scan`), the control channel, the
MCP server, a dependency, or the release workflow. The reporting policy is
[`../SECURITY.md`](../SECURITY.md). The data-safety rules (atomic writes, originals, merge never
replace) are in [`data-model.md`](data-model.md); this document is the adversarial view of the
same code.

> **Status:** each control below is marked **Implemented** (with its code path) or **Planned (Mn)**
> with the milestone from [`roadmap.md`](roadmap.md) in which it lands; as of M6 the file-system,
> decoder and watcher controls are built, and so are metadata reading, metadata write-back and the
> release workflow. M15's lines (videos, cameras, the OS services, the hand-off between two
> launches, sound, signing, the fuzz targets and the dependencies M15 added) were set by the 1.0
> audit (M15.36, 2026-10-20) from what each task's pull request and crate README record, each
> naming its task, and each saying what is not done. The commit that lands a control
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
| Metadata (EXIF/XMP/IPTC/ICC) | IFD count/depth/entry limits, cycle detection, XML depth/node limits, no DTDs or entities, segment reassembly limits; merge-preserving writer; fuzz targets | **Implemented** (M6: reading M6.4 and M6.9, the merge writers M6.10, M6.17 and M6.18, `.xmp` sidecars for RAW M6.19, the write queue M6.30, write-back from commands M6.33, reading on scan M6.32; `crates/meta`, `crates/store/src/metawrite.rs`; fuzz targets in `crates/meta/fuzz`) | only the owned fields are written, into JPEG, PNG, WebP and TIFF; RAW files never (an `.xmp` sidecar beside them, opt-in); a field another application changed meanwhile is imported, never overwritten (`xmp-interop.md` §5) |
| Folder sidecar `.dalica.json` | size/depth/entry limits, single-component names only, escaped values, unknown data preserved; fuzz + property tests | M1 (property tests); fuzz targets `parse`, `roundtrip` and `recipe` **implemented** (M15.39, §7.1) | a sidecar is as private as its folder |
| Legacy sidecars and backups | read-only fallback when a folder has no `.dalica.json`, same limits, no path from them is ever opened | M14 (the bounded reader **implemented**, M14.2: `crates/sidecar/src/legacy.rs`, fuzz target `legacy`) | |
| Library index, journal, settings | checksummed records, size and depth limits, `.corrupt-<time>` quarantine | M1 | |
| Directory walks | depth 32, 1,000,000 entries, symlink-loop detection, off the UI thread | M1 | |
| File writes | atomic replace with identity check, no writes through symlinks, OS trash only | **Implemented** for sidecars (M1) and metadata write-back (M6.30: re-read, merge, verify, `fs::replace` expecting the identity read, the modification time put back; a root checked right before each write, M6.43); M8 (Save), M9 (file operations) | network file systems weaken atomicity (§5.9); a process killed between the rename and putting the time back leaves the new file with the write's time |
| Control channel | loopback only, 256-bit token from a `0600` file, JSON-request-only lines, request/reply/connection/batch limits | M2 | no per-client capabilities, no encryption |
| MCP server | stdio by default, roots, read-only mode, tool annotations, same limits | M10 | an authenticated agent can still trash or caption any photo in the library unless read-only |
| Network features | user-initiated only, TLS with certificate validation always on, size limits, pinned model hashes | the HTTPS transport **implemented** (M13.13: `crates/fetch/src/http.rs`, `http/`, `proxy.rs`; the stack of §8.2, redirects to https only and at most 8, reply heads capped, `HTTPS_PROXY`/`NO_PROXY` or a manual proxy through `CONNECT`; fuzz target `response`); map tiles and the geocoder **implemented** (M13.20: `crates/engine/src/map_tiles.rs`, `geocode.rs`: off until the user allows them, only from the open Places pane, at most two tiles at a time, cached, one geocoder request a second, the limits of §4.2); model downloads M13.16 | the crypto provider is an alpha release (§8.2); no revocation checking (no OCSP or CRL fetching); the root certificates are fixed at build time; no proxy authentication |
| Generated output | escaped JSON, sanitized file names | M8 (export), M14 (collage) | |
| Dependencies | pure Rust, banned crates, licence allow-list, advisory check | M0 | scanner results do not replace review |
| Videos | our own bounded box and EBML walks before any decoder; frames and sound read sample by sample under the raster limits, every decoder call panic-guarded; fuzz target `video_probe` | **Implemented** (M15.2 probe, M15.15 frames and sound, M15.20 posters, M15.25 playback and Export Clip; §4.2) | the H.264 decoder (`altc264`) rests on 257 `unsafe` sites and HEVC is behind a non-default feature (§8.7, questions for M15.R); the frame and sound readers have no fuzz target yet (§7.1) |
| Cameras and phones (PTP/MTP) | our own reader of what a device sends, every count and length bounded, timeouts, copies verified, deletes only of verified copies, never over automation | **Implemented** (M15.9 reader, M15.19 import; §4.2, §5.9; fuzz target `ptp`) | Linux only: on Windows and macOS the system's own service holds the device (§5.9); no live session measured on hardware (checked at M15.R) |
| External programs, clipboard, drag out, default viewer | argument vectors, never a shell; launches only from the user's own input, never over automation; system handler lists read with limits; `mimeapps.list` merged through `fs` | **Implemented** (M15.12, M15.17, M15.22, M15.23, M15.24; §5.11) | dragging out on Wayland is not built; macOS sets no default viewer (it only declares Dalica a viewer); Windows and macOS are compiled and tested by CI only |
| Files handed to the running Dalica | loopback, a 256-bit token in a private file, one bounded request per connection, the paths only opened in the viewer | **Implemented** (M15.18; §6.1) | no fuzz target for its request line (§7.1) |
| Releases | draft releases, `SHA256SUMS.txt`, optional signing secrets, least-privilege jobs, signing and notarization, an SBOM and an advisory check per release | **Implemented** (M6.13 `.github/workflows/release.yml`, `packaging/checksums.sh`; packages M6.21–M6.23; Windows signing M15.10, macOS signing and notarization M15.13, SBOM and advisories M15.40; §10) | every dry run signs with a throwaway identity; 1.0 ships unsigned (maintainer decision at M15.R, 2026-10-20: no certificate or Apple Developer ID yet), so signing turns on when the `release` environment gets its secrets; consumers verify `SHA256SUMS.txt` |

The table is a plan, not a guarantee about any build. Verify source and release evidence before
relying on it.

### 1.2 Priorities

1. M0–M2: the control channel ships with authentication, line validation and limits from its first
   commit; there is no unauthenticated phase. The never-crash lints, `panic_hunt` (M1) and the
   dependency check (§8) land with the workspace.
2. M1: bounded decoders and walkers, the sidecar parser (its fuzz targets M15.39, §7.1), and one shared
   atomic-write primitive with the path rules of §5.
3. M6 and M8: metadata write-back with merge fuzzing; Save with originals backup.
4. M10: MCP roots, read-only mode, and server proof for the bridge (§6.3).
5. M13–M15: network responses, collage output, legacy sidecar reading, release signing and SBOMs.

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
  video containers, their H.264 / HEVC / AAC streams and music files (M15) / face models (M13)
  map tiles and geocoder replies (M13) / what a camera or phone sends over PTP/MTP (M15)
  system handler lists: desktop entries, mimeapps.list, the registry's answers (M15)

untrusted names
  file and folder names from disks, shares and devices / names inside sidecars / paths in metadata

untrusted control data
  CLI arguments / JSON-lines control requests / MCP requests / agent commands / snapshot scripts
  files handed over by another Dalica launch (M15)

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
  network replies (M13)
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
| JSON line → command | `apps/dalica/src/control/`, `crates/ui/src/control/`, `crates/engine` | auth, line shape, limits, command policy, parameter type/range, enabled state |
| MCP call → command | `crates/mcp` | roots, read-only mode, argument limits, result limits |
| Library → generated output | `crates/create` | JSON escaping, name sanitizing, external-process arguments |
| Video bytes → frames and sound | `crates/image` (`formats/video/`) | box and EBML limits, sample tables against their boxes, the coded size before the decoder, decode budget, panic guard (M15) |
| Camera → import | `crates/scan` (`ptp/`), `crates/engine` (`camera.rs`) | container lengths, handle and object limits, timeouts, verified copies, deletes only of verified copies, never over automation (M15) |
| Library → other programs | `apps/dalica` (`os/`), `crates/engine` (`cmd/os.rs`, `automation.rs`) | argument vectors, no shell, user input only, bounded handler lists, `mimeapps.list` merged through `fs` (M15) |
| Another launch → running app | `apps/dalica` (`instance/`) | loopback, token, one bounded request, paths only opened in the viewer (M15) |

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
| Raster decode (all formats) | `image` | edge ≤ 65,535 px; ≤ 2³⁰ pixels; decoded allocation charged to the memory budget (RAM/4, 256 MiB–1.5 GiB): a file whose full decode does not fit is decoded at reduced scale (JPEG DCT scaling, embedded preview) or shown as a "too large" placeholder | **Implemented** (M1, M5.1): `Limits::check_size` before any decoder runs (`decode.rs`), every pixel buffer reserved fallibly through one `Budget` per decode (`limits.rs`), each format module's working buffers charged to it first, third-party decoders inside `catch_unwind` ("Damaged file"); JPEG DCT scaling not yet (past 64 MP a thumbnail comes from the embedded one) |
| TIFF strips and tiles | `image` | our own IFD walk first (the IFD limits below); ≤ 2²² strips or tiles, both offset lists inside the file; the decoded size, the raw size and the `tiff` decoder's own memory (64 B per chunk, the largest compressed chunk, a decoded copy for JPEG strips) charged to the budget before `tiff` reads a byte | **Implemented** (M5.6; `formats/tiff.rs`) |
| WebP, GIF, BMP | `image` | our own RIFF chunk walk, GIF block walk to the first image descriptor and BMP header read first; a frame's bitstream checked against its canvas before `image-webp` allocates; the decoders' working buffers (3–9 bytes per pixel) charged to the budget | **Implemented** (M5.7; `formats/{webp,gif,bmp}.rs`) |
| JPEG XL | `image` | header read from a growing prefix with a 64 MiB allowance; the decode's remaining budget lent to `jxl-oxide`'s allocation tracker, samples carved from what it has not used; no thread pool. An embedded ICC profile's declared sizes are read first (with the decoder's own header and entropy-code parsers): a profile over the ICC limit (16 MiB), or an encoded stream over it plus the decoder's 64 KiB slack, is "too large" before the decoder reserves either (it reserves them outside its tracker); a smaller one is charged to the header's allowance and to the decode's budget (the stream and two copies of the profile) | **Implemented** (M5.8, M6.41; `formats/jxl.rs`) |
| ISOBMFF (HEIF, AVIF, CR3) | `image` | our own box walk before any decoder: nesting ≤ 12, ≤ 4,096 boxes per level, every box counted, a box past its parent is malformed; HEIF through `heic-rs` (pinned exactly, panic-guarded, header and decode) only with the `heif` feature | **Implemented** (M5.10; `formats/bmff.rs`, `formats/heif.rs`) |
| RAW containers | `image` | CR2/NEF/ARW/DNG/PEF/ORF/RW2/SRW through the IFD walk (SubIFDs, EXIF IFD, maker-note pointers with their own caps); CR3 boxes as above; CRW heaps nesting ≤ 8; RAF, MRW and X3F directories bounded; ≤ 64 preview candidates, each ≤ 64 MiB and inside the file | **Implemented** (M5.9, M5.10; `formats/raw/`) |
| Animated GIF/WebP | `image` | first frame for thumbnails; ≤ 10,000 frames and the same allocation budget for playback | **Implemented** for display (M5.7: frame one only; a WebP with more than 10,000 `ANMF` chunks is damaged); playback M11 |
| PSD | `image` | flattened composite only; layers are never parsed (only the layer section's length is read); every length and count checked against the data; the resource walk counts its steps | **Implemented** (M5.8; `formats/psd.rs`) |
| Embedded thumbnail / RAW preview | `image` | offset + length inside the file; ≤ 64 MiB; decoded under the raster limits | **Implemented** (M1 EXIF thumbnails, M5.8 PSD resource, M5.9–M5.10 RAW previews; `Limits::max_thumbnail`) |
| EXIF/TIFF IFDs | `meta` | ≤ 1,024 IFDs, depth ≤ 10, ≤ 8,192 entries per IFD, visited-offset cycle detection (LightCraft); maker notes ≤ 64 IFDs, depth ≤ 3; strings ≤ 64 KiB; zero-denominator rationals are invalid values | **Implemented** (M6.4; `exif/read.rs` through `dalica-image`'s IFD walker: IFD0, EXIF, GPS, Interoperability and the next-IFD chain; strings kept to 64 KiB; maker notes are not parsed yet, only listed by size) |
| XMP | `meta` | packet ≤ 16 MiB after extended-XMP reassembly (matching GUID, chunk offsets inside the declared length); XML depth ≤ 64, ≤ 200,000 nodes (LightCraft); `DOCTYPE`, entity declarations and external references rejected | Reassembly **implemented** (M6.4; `container/jpeg.rs`: the GUID the main packet declares, one declared length, chunks inside it and covering it, standard + extended ≤ 16 MiB; compressed PNG text inflated to ≤ 16 MiB); the XML limits **implemented** (M6.9; `xmp/xml.rs`, our own tokenizer: depth ≤ 64, ≤ 200,000 nodes counting elements, attributes, text runs, comments, processing instructions and CDATA sections; any `<!DOCTYPE` or other declaration refused, only the five predefined entities and character references to XML characters accepted; prefixes resolved and repeated attributes found in linear time) |
| IPTC (IIM in APP13 / 8BIM) | `meta` | resource and dataset lengths inside the segment; dataset ≤ 64 KiB; ≤ 10,000 datasets | **Implemented** (M6.4; `iptc/read.rs`; also ≤ 4,096 image resources) |
| Metadata write-back | `meta`, `store` | owned fields only; a file whose EXIF, XMP, IPTC or Photoshop block cannot be read is not rewritten, nor is a TIFF-structured RAW; JPEG: EXIF ≤ 65,527 bytes and the XMP packet ≤ 65,504 bytes in one APP1 (only padding is dropped to fit, else the write fails naming the field), APP13 split at the segment limit; IPTC caption cut at 2,000 bytes and keywords at 64 on a character boundary; PNG chunks < 2³¹ bytes, the WebP RIFF and TIFF offsets within 32 bits; TIFF written append-only, and refused when IFD0 or a rewritten value overlaps any other IFD, value, strip or tile; every result re-parsed and verified before it is written; the file read whole ≤ 1 GiB | **Implemented** (M6.10, M6.17, M6.18: `iptc/write.rs`, `exif/edit.rs`, `write/{jpeg,png,webp,tiff}.rs`, `write/mod.rs` `verify`; M6.30: `crates/store/src/metawrite.rs` `MAX_FILE`) |
| `.xmp` sidecars for RAW | `meta` | the sidecar ≤ the XMP limit, read with the XML limits above; one that is not UTF-8, not XML within the limits or has no `rdf:RDF` with content is damaged: read falls back to the embedded packet, a write keeps it aside as `.xmp.corrupt-<time>`; every merged sidecar checked well-formed before it is returned; names resolved from the folder's own listing, never from a path inside a file | **Implemented** (M6.19; `xmp_sidecar.rs`; written by M6.30) |
| Metadata read on scan and for Properties | `scan`, `engine` | a JPEG read only up to its image data (256 KiB, doubled while the metadata runs on); PNG, WebP, TIFF and TIFF-structured RAW read whole ≤ 1 GiB; each read inside `catch_unwind`; the Properties cache ≤ 256 answers, ≈ 64 MiB of text, 64 queued requests, 2 reader threads | **Implemented** (M6.32 `crates/scan/src/meta.rs`; M6.16 `crates/engine/src/metadata_cache.rs`) |
| ICC profile | `image` | APP2 chunk sequence validated; ≤ 16 MiB; tag count ≤ 1,000; LUT sizes bounded by the colour library | **Implemented** (M5.1; `color.rs`: over the limits or inconsistent, a profile is unusable and the pixels show as untagged) |
| `.dalica.json` | `sidecar` | file ≤ 64 MiB; nesting depth ≤ 16; string ≤ 1 MiB; ≤ 1,000,000 photo entries; ≤ 1,000 keys per entry; ≤ 10,000 operations per recipe; ≤ 1,000 regions per operation; list entries ≤ 100,000 | M1; the M6 fields' caps **implemented** (M6.2, M6.8: caption and folder description ≤ 16 KiB, a tag ≤ 256 bytes, ≤ 1,000 tags per photo, a place ≤ 1 KiB, ≤ 10,000 operations per recipe and ≤ 64 KiB per operation, `crates/library/src/tags.rs`; a longer text is shown cut and kept whole in the file, a recipe past them is not used and kept; a value of the wrong type is reported once and kept) |
| Legacy sidecars | `sidecar` | file ≤ 64 MiB; line ≤ 1 MiB; ≤ 1,000,000 sections; ≤ 1,000 keys per section; the recipe limits above; encoding detection never fails the whole file | The reader **implemented** (M14.2; `crates/sidecar/src/legacy.rs`, `legacy/`: `LegacyLimits`, a file past one refused naming it (`LegacyError`); duplicate sections and keys merged, garbage sections skipped; UTF-8 when the file says so, else Windows-1252, bytes that do not decode shown as U+FFFD and marked lossy; a text overlay's record ≤ the line limit; fuzz target `legacy`); the recipe limits apply where M14.3 maps the edit list |
| `.dalica-collage` recipes | `create`, `engine` | file ≤ 1 MiB (read with that limit: a longer file is refused before it is parsed); nesting depth ≤ 32; ≤ 1,000 clips, none twice; a clip's path ≤ 32 KiB; every number finite and in range (the seed and shuffle as 16 hex digits, a page format's sides, the output ≤ 16,384 px a side); a newer version refused; a path in it only compared with the photos already indexed (§5.5) | **Implemented** (M14.4: `crates/create/src/collage/recipe.rs`, `crates/create/tests/collage_doc.rs`; M14.17: Edit Collage and Can Edit read it on a worker under that limit, a hostile recipe failing the job without a panic, `crates/engine/src/collage_output.rs`, `crates/engine/tests/collage_output.rs`) |
| JSON files (`folders`, `albums`, `people`, `settings`) | `library` | file ≤ 64 MiB; depth ≤ 64 | M1 |
| Index journal / snapshot | `library` | checksummed records ≤ 16 MiB; a torn tail is discarded, a bad checksum stops replay at that record; version checked | M1 |
| Control / MCP JSON | `ui`, `mcp` | see §6.5 | M2 / M10 |
| Video containers (probe) | `image` | MP4, M4V and MOV through the ISOBMFF box walk above (only `ftyp` and `moov` read, a movie box over the 64 MiB header limit refused); Matroska through our own EBML reader (RFC 8794, RFC 9559): nesting ≤ 12, ≤ 4,096 elements per level, an element past its parent malformed, unknown-size elements bounded by their parent; a cut file is `Truncated`, never a panic; HEIC, AVIF and CR3 are never videos | **Implemented** (M15.2: `formats/video.rs`, `formats/video/{mp4,ebml,mkv}.rs`, `crates/image/tests/video.rs`; fuzz target `video_probe`, 7.6 M runs in 181 s, one finding fixed with its regression test). Reserved decision 2 chose decoders in the process (M15.46's record, `video-decoders.md`), so no external video process exists |
| Video frames and a clip's sound | `image` | our own sample-table and cluster demuxers: ≤ 2²¹ samples per track, a sample ≤ 32 MiB read one at a time (a clip is never held in memory), every table checked against its box before anything is reserved, codec configuration ≤ 64 KiB; every SPS parsed and its coded size checked before the decoder sees it (≤ 8,192 a side, ≤ 8,192 × 4,352 px, the caller's `Limits`, and 20 pictures plus 8 per decoding thread within the decode budget); ≤ 8 threads, spawned fallibly; sound ≤ 8 channels, ≤ 192 kHz, ≤ 4,096 frames a packet; every decoder call inside `catch_unwind` (the decoder rebuilt after a panic) | **Implemented** (M15.15: `formats/video/frames.rs`, `frames/track.rs`, `video/sound.rs`, `crates/image/tests/video_frames.rs`: cut, bit-flipped and oversized streams refused with their reasons under a time bound). H.264 through `altc264` in every build; HEVC through `rusty_h265` only with `dalica-image`'s non-default `hevc` feature, which neither `dalica-engine` nor the apps forward, so the shipped build shows HEVC clips as tiles; the decoders' own `unsafe` is §8.7's. No fuzz target reads frames or sound yet (§7.1) |
| Slideshow music files | `image` | known by their first bytes, never their name; a file over 2 GiB refused before it is read; ≤ 8 channels, a rate of 8–192 kHz, ≤ 65,535 frames a block; read as a stream; a decoder panic caught and answered as a damaged file | **Implemented** (M15.27: `formats/audio.rs`, `crates/image/tests/audio.rs`); MP3, Ogg Vorbis, FLAC and WAV through `nanomp3`, `lewton`, `claxon` and `hound` (§8.7) |
| Cameras and phones over PTP/MTP | `scan`, `engine` | every container's length checked against what arrived; ≤ 5 parameters, ≤ 100,000 object handles, ≤ 4,096 codes in a dataset, strings ≤ 255 UTF-16 units; an object ≤ 4 GiB and never read past the size its info declares (chunked reads end with a one-byte read past the end); 5 s per transfer, 60 s per transaction, an unplugged or silent device refused without a panic; names through `core::names::sanitize`; the import reads an object whole into memory only up to 64 MiB, writes every copy through `fs` (temporary file, fsync, read back against the received bytes' hash, a rename that never replaces), deletes from the device only an object it copied and verified while the device still reports its listed size and dates, only after the question and never over automation; no device I/O on the owner thread; a held device is listed as held, never detached | **Implemented** (M15.9: `crates/scan/src/ptp.rs`, `ptp/`, `crates/scan/tests/ptp.rs` over an in-memory responder, fuzz target `ptp` 5.7 M runs in 181 s; M15.19: `crates/engine/src/camera.rs`, `crates/engine/tests/import_camera.rs`). The USB transport (`nusb`) builds on Linux and Windows only; no live session was measured on hardware (checked at M15.R) |
| Files handed to the running Dalica (§6.1) | `dalica` | one request line per connection ≤ 525,312 bytes within 5 s in all; ≤ 64 paths, each absolute UTF-8 ≤ 4,096 bytes, any violation refusing the whole request; reply ≤ 256 bytes; `instance.json` ≤ 4 KiB | **Implemented** (M15.18: `apps/dalica/src/instance.rs`, `instance/wire.rs`, `apps/dalica/tests/instance.rs`); no fuzz target |
| System handler lists | `dalica` | desktop entries, `mime/globs2`, `mime/subclasses` and `mimeapps.list`: ≤ 256 KiB per file, ≤ 8,192 entries, ≤ 4 folder levels, ≤ 64 handlers; an unclosed quote in `Exec` refused; a field code inside a longer word dropped, never filled; Windows' `reg query` output parsed by the same bounded reader | **Implemented** (M15.12: `apps/dalica/src/os.rs`, `os/`, `apps/dalica/tests/os.rs` `hostile_entries_and_lists_are_bounded`, 2,000 random inputs through every parser, 20,000 at full strength) |
| `mimeapps.list` written (default viewer) | `dalica` | written through `fs`, merged: every other group, comment and key kept byte for byte; the replaced handlers remembered first in Dalica's own file and put back on "Don't use Dalica as a viewer", leaving a key the user changed since alone; a list that is not UTF-8 kept aside as `.corrupt-<time>`; a list over the size limit or a link refused and left untouched | **Implemented** (M15.24: `apps/dalica/src/os/default_viewer.rs`, `apps/dalica/tests/default_viewer.rs`: 300 generated hostile lists and a 100 KB line) |
| Face models | `faces` | file ≤ 512 MiB (a longer source refused before a byte is read); weights ≤ 64 Mi elements in all, ≤ 20,000 weight tensors and ≤ 20,000 nodes; rank ≤ 8 for every weight, declared input or output and value; names ≤ 4,096 bytes, lists ≤ 1,000,000 values, ≤ 5,000,000 protobuf fields; one input, ≤ 64 outputs; any one tensor ≤ 64 Mi elements; live tensors, constant copies and the convolution scratch kept between layers ≤ 512 MiB at every step, checked for the input before anything is allocated; every weight and every computed value finite (an overflow is an error); stride and padding ≤ 65,536, nearest `Resize` by whole factors 1–16; external or sparse weights, custom operator domains, training graphs, opsets outside 10–17 and element types other than float32 (int64 only for constant shapes) refused by name; the interpreter starts no thread and a panic in a run is caught; SHA-256 pinned in the source for the built-in models | Limits **implemented** (M13.6; `crates/faces/src/limits.rs`, `graph.rs`, `net/shape.rs`, `runtime.rs`); the pins M13.10 |
| HTTPS replies | `fetch` | status line and headers ≤ 64 KiB and ≤ 100 lines (no folded lines), ≤ 4 interim `1xx` heads, a chunk-size line ≤ 1 KiB and ≤ 100 trailers; one `Content-Length` (two different ones refused); a compressed or transformed body refused; ≤ 8 redirects, each to an https address; a proxy's `CONNECT` answer ≤ 16 KiB; the body capped by the download's spec (`fetch`) | **Implemented** (M13.13: `crates/fetch/src/http/response.rs`, `proxy.rs`) |
| Map tiles, geocoder replies | `engine` | tile ≤ 1 MiB (refused as it arrives, announced or not) and ≤ 1,024 px edge (read from its header before any pixel), decoded under the raster limits, a placeholder otherwise and never kept in the cache; JSON reply ≤ 1 MiB, nesting depth ≤ 32 (checked before it is parsed), ≤ 20 results, a name ≤ 256 bytes and a place ≤ 1 KiB each, a point on the globe or the result is passed over; the cache (`<cache>/tiles/`) ≤ 200 MB, pruned oldest first, listing and deleting only its own file names inside its own folder names, links never followed | **Implemented** (M13.20: `crates/engine/src/map_tiles.rs`, `geocode.rs`; `crates/engine/tests/map_tiles.rs`) |

### 4.3 Work budgets during scans

- Each file's decode and metadata read runs inside the per-file `catch_unwind` guard on a worker
  thread with a cancel flag. **Implemented** for metadata reads (M6.32: the scan's reads in
  `crates/scan/src/meta.rs`, the cancel flag checked between files; M6.16: the Properties pane's
  reads on their own threads, `crates/engine/src/metadata_cache.rs`); decoders from dependencies
  run inside `catch_unwind` ("Damaged file", §4.2).
- A file that fails, panics or exceeds a 10-second work budget is recorded by (path, size, mtime)
  and not retried until it changes; the tile shows a generic placeholder and the scan continues.
  **Implemented** for the scan's header reads (M5.4: `crates/scan/src/walk.rs`, `BUDGET` and
  `Failure`, given back to the next scan in `Options::failures`; the session keeps them for its
  lifetime, not across restarts, `crates/engine/src/scanning.rs`); a thumbnail decode that fails
  or panics draws the placeholder for good (`crates/engine/src/thumbs.rs`, "Damaged file"), with no
  time budget of its own.
- Work counters (not only byte counts) for highly fragmented but size-bounded files: number of
  JPEG scans, Huffman tables, PNG chunks, TIFF strips/tiles. **Implemented** in `dalica-image`
  (M5.1–M5.10: `Work` counts JPEG segments, PNG chunks, APP2 chunks, IFDs, ISOBMFF boxes and
  container directories; TIFF strips and tiles are capped at 2²², WebP frames at 10,000).

### 4.4 Tainted text

Text that came from a file, a sidecar, a device or an agent:

- is never interpreted: not as a path (§5.5), a format string, a shell command, HTML or a URL;
- is displayed with bidirectional-override and other invisible control characters made visible in
  file names (a name like `photo<RLO>gpj.exe` must not pass for a JPEG);
- is escaped for its destination: sidecar values are written through the JSON serializer, so a
  caption can never start a new key or object; XMP writing escapes XML and drops characters that
  are invalid in XML 1.0; IPTC writing respects the dataset length and the declared character set.
  **Implemented** for the metadata writers (M6.9 `xmp/merge.rs`, M6.10 `iptc/write.rs`: 1:90 set
  to UTF-8, Latin-1 text re-encoded; M6.17 `fields::clean_text` also drops U+FFFE and U+FFFF);
- is kept out of logs: no captions, tags, people names, face data or tokens; paths only where
  needed and never in bulk. **Implemented** for M6's paths: settings problems are reported by key
  (M6.5), errors from tag and caption commands and from metadata verification name the field,
  never its value (M6.2, M6.4, M6.15), an IPTC cut logs one line without the text (M6.10), and a
  failed metadata write's notice names the file and the field (M6.33);
- is passed to external programs only as separate argv elements (§5.11);
- is returned to AI agents as data. MCP tool descriptions state that captions, tags and file names
  are user content and may contain instructions that must not be followed. **Planned (M10).**

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
`dalica-fs` `atomic::replace`), with a byte-for-byte check in step 6; for metadata write-back
(M6.30, `crates/store/src/metawrite.rs`) `dalica-meta`'s `verify` first checks the new bytes
against the old (the fields written read back, every byte outside the rewritten blocks unchanged),
then `fs::replace` writes them expecting the identity it read.**

1. Read the target and record its identity: device and inode (volume serial number, file index
   and creation time on Windows), size and modification time (§5.3 says what it catches).
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
`Expect::Unchanged`). Save's checks built (M8.2: `dalica-fs` `originals::back_up` re-checks the
photo's identity right before one rename that never replaces moves it into the backup; a backup
that exists must be the recorded one, its size and the record owner's digest checked over its
bytes, and then nothing moves; any other backup is refused with nothing moved. `originals::restore`
(Undo Save, Revert) moves the backup back only while it is still the recorded one and the saved
file is still the version recorded, `Expect::Unchanged`, or absent after a Save that stopped before
writing it, `Expect::Absent`). The Save commands that call them are planned (M8).**

**What the identity sees** (`dalica-fs` `Identity`; the same on every OS since M8.27). A version of
a file is the object on disk plus its size and modification time:

- **Unix** (Linux, macOS): device and inode from `stat`/`fstat`, at no extra cost.
- **Windows:** volume serial number, file index and creation time, read with
  `GetFileInformationByHandle` (through `winapi-util`, a safe wrapper; no `unsafe` in Dalica) from
  a handle: the file Dalica has open, or the name opened with no data access, every sharing mode
  and without following a final link or junction, so the look never blocks another program. Size
  and modification time come from the same call. Until M8.27 the creation time stood in for the
  file index (stable Rust exposes no index), and NTFS "tunnels" a replaced file's creation time
  onto a file renamed over its name within about 15 seconds, so a same-size replacement that kept
  the modification time looked unchanged; the file index tells it apart.

So another program's rewrite through a new file renamed over the name (editors, sync clients,
`exiftool`) is always a new identity; so is any change of size or modification time. What the
guard cannot see, on any OS: an edit made in place, through the same file, that keeps the size and
puts the modification time back; it is a guard against ordinary rewrites, not a content hash. Two
names of one file (hard links) have one identity. On volumes whose file index is not unique or
not stable (FAT and exFAT may renumber a file, ReFS ids are 128-bit, some network shares report
0), the creation time, size and time still count, and an index that changes on its own makes Dalica
refuse the write as changed (it re-reads), never write over a file it did not read.

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
**The collision loop built (M8.2: `dalica-fs` `create::new_file` writes a temporary file, then
gives it the first free name of `core::names::candidates` in one step that never replaces a file:
`renameat2(RENAME_NOREPLACE)` on Linux and Android, `renameatx_np(RENAME_EXCL)` on Apple systems,
a hard link then an unlink of the temporary name on other systems, Windows included, and where the
volume refuses the flag; on a volume without hard links the bytes go straight into the first name
exclusive creation gives. `create::new_folder` makes `<name>`, `<name>-001`, … with `mkdir`, which
fails on a taken name. Both refuse a name that is not one plain component, with nothing written:
separators, `.`/`..`, drive and stream prefixes, reserved device names, Dalica's own `.dalica*`
and temporary names, control characters, a trailing dot or space, more than 255 bytes); the
sanitizer is built too (M8.13: `dalica-core` `names::sanitize(name, fallback)` removes the
separators, `: * ? " < > |`, control characters and the bidirectional marks, embeddings,
overrides and isolates (U+061C, U+200E–U+200F, U+202A–U+202E, U+2066–U+2069); strips leading
dots, dashes and white space and trailing dots and white space; cuts to 255 bytes on a character
boundary; gives a Windows device name (`CON`, `nul.txt`, `COM1`, `LPT¹`, `CONIN$`, … in any case)
a leading `_`; and takes the fallback, sanitized the same way, then `Untitled`, when nothing is
left. Its result always passes `create::new_file`'s check, and a second pass changes nothing
(property-tested). Export uses it for the export folder's name and each exported photo's stem;
import folders (M11) will.) Collage files are built (M14.17): `Collage 2026-10-19.jpg` through
`names::sanitize`, then the JPEG and its `.dalica-collage` recipe each written by `fs::exact_file`
(never replacing a file) under the first pair of `names::candidates` free for both
(`Collage 2026-10-19-001.jpg` and `Collage 2026-10-19-001.dalica-collage`); a pair half taken is
passed over, and a JPEG whose recipe name was taken between the look and the write is removed
again, so a picture never sits beside another collage's recipe
(`crates/engine/src/collage_output.rs`, `crates/engine/tests/collage_output.rs`).**

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
embedded, never loaded by a referenced name. **Planned (M1, M14).**

### 5.6 Symlinks, junctions and hard links

- Directory walks follow directory symlinks and junctions with (device, inode) loop detection
  inside the depth and entry bounds of `data-model.md` §7. **Built (M1.3: `dalica-fs` `walk`;
  folder links are followed after the real tree, so a real folder always wins over a link to it).**
- **Writes never go through a link.** Sidecars, photos on Save or metadata write-back, the
  `.dalica-originals` directory and its contents are opened without following a final symlink
  (`O_NOFOLLOW`-style). **Built for the originals backup (M8.2: `.dalica-originals` is opened or
  made relative to the photo's held folder with `O_DIRECTORY | O_NOFOLLOW`, a link or a file in
  its place is refused with nothing moved, and a link in place of the photo or of its backup is
  refused too; the move is `renameat` between the two held handles, so a backup folder swapped for
  a link mid-move leaves the photo in the folder that was opened).** If the target is a link, or a folder reached through a link resolves
  outside every watched root, Dalica does not write there: the folder is treated as read-only and
  shows a `caution` notice. The exact UI and how user data for such folders is kept are settled in
  M1 (`data-model.md` §7). **Planned (M1).**
- Moves and renames inside Dalica operate on the link, not its target, and never follow a link
  out of the source or destination folder. **Built in `fs` (M9.2: `dalica-fs` `transfer` refuses
  a link in place of either folder, of the file or folder being moved, or inside a folder tree
  copied across volumes, with nothing moved; both folders are opened once and held without
  following a link, so a folder swapped for a link before the opening is refused and one swapped
  after it leaves the move in the folder that was opened. `trash::send` sends a link to the trash
  as itself, never its target).** The commands that call them are built (M9.13–M9.15: moving,
  copying, renaming and deleting photos and folders, `crates/engine/src/cmd/{files,rename,delete,folder_ops}.rs`).
- Where the platform allows, the write path holds a directory handle and works relative to it,
  so a component swapped for a link after validation cannot redirect the write. Canonicalization
  alone is insufficient because links can change after the check. **Built (M5.3: `dalica-fs`
  `atomic::replace` through its internal `dirfd` module).** On Unix the target's folder is
  validated, opened once (`O_DIRECTORY | O_NOFOLLOW`, through `rustix`'s safe API) and checked to
  be the folder that was validated (device and inode); the exclusive temporary `openat`, write,
  `fsync`, identity re-check (`fstatat` without following), `renameat`, folder `fsync` and the
  read-back (`O_NOFOLLOW`) all work relative to that handle, so a swap after validation leaves the
  write in the original folder, and a swap before the open is refused. A folder whose own name is
  already a link when the write starts (a watched folder linked to another drive) is followed once
  at validation and then held the same way; whether such a folder may be written is the store's
  decision (the bullet above). Windows has no handle-relative calls on stable Rust, so it compares
  the folder's identity again before each step that creates, renames or removes a file, refuses a
  folder that turned into a link or junction, and compares the file's identity before and after
  opening it. Its folder identity is the volume serial number, file index and creation time
  (§5.3, M8.27), so a folder replaced by another real folder of the same name is detected there too.

### 5.7 Trash, never delete

- Deleting a photo, video or folder moves it to the **OS trash** after confirmation
  (`data-model.md` §8). Dalica has no permanent-delete path for library content, in the UI or over
  automation. **Built in `fs` (M9.2: `dalica-fs` `trash::send` hands a file or folder to the
  `trash` crate, called under `catch_unwind`: the freedesktop.org trash on Linux and the BSDs, the
  Recycle Bin through the shell on Windows, `NSFileManager` on macOS, never the Finder.
  `trash::restore` brings items back to their old path on Linux and Windows, found by that path
  and, where the trashed object can be looked at, by its identity, and never replaces what is at
  the old path; macOS answers `Unsupported`, so undoing a delete there is disabled with its
  reason).** **The delete commands are built (M9.15: `photo.delete` and `folder.delete`,
  `crates/engine/src/cmd/delete.rs`): file operations that ask first (without `"confirm": true`
  they refuse with the question, unless "Delete from disk without asking" is on), send each photo,
  or the folder with everything in it, to the trash through `trash::send` on the job's thread,
  refuse a volume root and the home folder, and keep everything the library said about what they
  trashed in the one undo step, which restores from the trash on Linux and Windows. Nothing over
  automation or in the UI reaches a permanent delete.**
- If the platform trash is unavailable for a volume (some network shares and removable media), the
  delete fails with an explanation that points to the file manager. It never falls back to a
  permanent delete. **Built in `fs` (M9.2: `trash::send` answers `Sent::NoTrash` with the item in
  place. On Linux and the BSDs Dalica first checks that a trash on the item's own volume takes it
  by a rename: the home trash when it is on that volume, else the volume's `.Trash/<uid>` or its
  `.Trash-<uid>`, made owner-only when missing; the crate's own fallback, a copy into the home
  trash on another volume and a delete, is never reached that way. One gap remains: a volume the
  mount table does not list on its own, such as a btrfs subvolume inside another mount, is trashed
  where the crate's mount table says. On Windows the shell is asked to warn instead of deleting
  permanently, cannot show the warning, and aborts; on macOS `NSFileManager` refuses).** The
  explanation is the delete command's, built (M9.15: the item fails naming the file, says the
  drive has no trash and that Dalica never deletes permanently, and points to the file manager; the
  file stays).
- "Remove from Dalica", "Rebuild library" and cache pruning never touch photos. Caches list and
  delete only files of their own naming shape. Stale sidecar entries are pruned, not files.
- **Camera and card import** ("Delete copied files from the card"): cards usually have no trash,
  so this is the only permanent deletion Dalica performs. They
  happen only after every copied file has been verified by hash at its destination, only for files
  the import listed, after an explicit confirmation, and they are refused over automation.
  **Built (M11.6: `dalica-fs` `remove::remove_copied`, which removes one source only while it
  still has the identity it had when it was copied and its copy still has the identity the
  verified copy returned, inside the folder listed and through no link, to the volume's trash
  when it has one and deleted only when the trash says the volume has none; M11.8:
  `import.removeFromSource` in `crates/engine/src/import_run.rs`, which offers only the files the
  last import from a card or drive copied and verified, never a duplicate or an excluded file,
  refuses without `"confirm": true`, disables Undo Import from its start (once a file is removed,
  for good: "The originals were removed from the card."), and is refused by every automation
  surface whatever it was granted, `confirm` and `--automation-allow-external` included:
  `automation::NEVER_AUTOMATED`, asked through `automation::never_automated` by MCP's
  `Policy::check_effect` and by the control channel's effect check (`crates/ui/src/control/engine.rs`
  `external_allowed`), both answering `external_denied`. Over automation `import.run`'s
  `destination` keeps to the write roots or the watched folders, as an export's folder does.
  M11.20: the same holds on the control channel's UI routes. While a request runs (until its
  reply, across the frames its input plays) the UI thread is marked as driven by automation, and
  the Import room asks `control::refuse` before it runs either command: Remove Imported Files is
  refused whether pressed by `ui.dialog.confirm`, `ui.clickWidget`, a raw `ui.pointer` click or a
  key, and Import All / Import Selected keep to the write roots, whatever `ui.set` drafted
  (`engine.execute` of them resolves the drafted folder first). Only the person's own click
  removes.)**
- Moving across volumes is copy + verify (hash) + delete-to-trash of the source, never delete
  before verify. **Built in `fs` (M9.2: `dalica-fs` `transfer` copies into a temporary file in the
  destination, `fsync`s it, keeps the source's modification time (and its permissions on Unix), compares it byte
  for byte with the source (stronger than a hash, and the source is on disk to compare with), gives
  it its name by a rename that never replaces, then checks the source unchanged and the copy still
  the verified one before the source goes to the trash; a volume without a trash keeps the source
  beside its copy and says so. A folder is copied the same way file by file into a temporary folder
  beside the target, bounded in depth and entries, then given its name. On one volume a move is one
  rename that never replaces, after the source's identity is checked right before it; a taken name
  is `Exists` for the caller to rename or skip, also when the file appeared between the check and
  the rename).** The move commands are built (M9.13: `photo.moveTo`, `photo.moveToNewFolder`;
  M9.15: `folder.move`).

### 5.8 Temporary files

- Atomic-replace temporaries live next to their target, named `.<name>.dalica-tmp-<random>`,
  created exclusively. Dalica removes only stale files of exactly this shape (older than 24 hours)
  when it next scans the folder. **Names built (M1.2: `atomic::is_temp_name`); the cleanup built
  (M5.3: `dalica-fs` `remove_stale_temps(folder, now)`: regular files of exactly that shape whose
  modification time is more than 24 hours before `now`, in that folder only, through the held
  folder handle; links, folders, near-miss names and other files are never touched; it reports
  what it removed and what it could not). Called when a folder is scanned (M5.17).**
- Other scratch files (render and export scratch, decoded previews spilled to disk) go to
  `<cache>/Dalica/tmp/`, created `0700` on Unix, with random names, never to a predictable name in
  a shared system temp directory. **Planned (M7).**
- New files (`create::new_file`, M8.2) use the same temporary shape, so a crash before the new
  name is given leaves only a stray temporary file that this cleanup removes; on a volume without
  hard links a crash can leave a torn file under the new name, never under an existing one.
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
  validated, and sizes reported by the device are not trusted for allocation. **Built for cards,
  drives and folders (M11.5: `dalica-scan` `import::list_source` walks through `dalica-fs` with its
  depth and loop bounds and at most 100 000 files; FIFOs, sockets and devices are never opened; a
  name that is not valid Unicode is shown in a lossy form with control and bidirectional characters
  replaced, and the file is only ever opened by its own name; each file is read in 64 KiB chunks,
  at most 256 MiB kept for its header and thumbnail and 16 GiB read in all, never by the size the
  device reports; headers decode under `dalica-image`'s limits, and a header that lies about its
  size is listed with its reason and no thumbnail; `crates/scan/tests/import_source.rs`). Cameras
  and phones over PTP/MTP: built (M15.9, M15.19) with the limits of §4.2: what the device sends is
  bounded and timed, and nothing it reports sizes an allocation past 64 MiB.** What each system
  lets Dalica reach (M15.9, `crates/scan/README.md`): on Linux the USB transport reaches a camera
  the desktop does not hold, and a phone the desktop already mounted (gvfs, kio-fuse) is listed as
  a drive; on Windows the portable-device driver and on macOS the image-capture service hold the
  device, so Dalica lists it as held and reaches it only as a drive or card when it offers one (no
  USB transport is built on macOS). Never detaching a driver is deliberate. Whether to reach
  cameras on Windows and macOS through the systems' own APIs (which needs `unsafe` in Dalica's
  code) is a question for M15.R.

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
  **Implemented (M15.12:** `apps/dalica/src/os.rs`, `os/`: reveal through the file manager's
  D-Bus `ShowItems` or `xdg-open` on Linux, `explorer.exe /select,` on Windows, `open -R` on
  macOS; Open With from the desktop entry's `Exec` split by its specification (a field code inside
  a longer word is dropped, never filled), Windows' `open` command split as
  `CommandLineToArgvW` does with `%1` put in after the split, `open -a` on macOS; the external
  editor gets every file in one start. `apps/dalica/tests/os.rs`
  `every_oss_vectors_keep_each_path_one_argument_and_never_start_a_shell` runs every OS's vectors
  through a recording runner with paths holding spaces, a leading `-`, non-ASCII and shell
  metacharacters; on the OS it runs on, stub programs on `PATH` record what they were given.
  Windows and macOS vectors are checked on Linux as data; their real starts are compiled and run
  only by the review's CI.**
- Only the user's own input launches external programs (Open With, Open in External Editor,
  Locate on Disk, a folder's Locate on Disk). **Implemented (M15.17, M15.22):** the four launching
  commands are in `automation::NEVER_AUTOMATED` and refused with `external_denied` over every
  automation route, even with `--automation-allow-external`
  (`crates/engine/tests/os.rs`
  `over_automation_the_launching_commands_are_refused_and_nothing_is_recorded`); a control
  request's synthetic key or click that reaches one in the window is refused by `App::run` while
  the request drives the app (`crates/ui/src/control/tests.rs`
  `a_control_requests_key_or_click_never_locates_on_disk`); choosing the external editor
  (`files.externalEditor`) is a launch by proxy and needs `--automation-allow-external`
  (`automation::EXTERNAL_OPTIONS`). The system clipboard (`photo.copyToSystem`,
  `photo.copyPath`) is not a launch and passes the policy; a picture is checked as width × height
  × 4 bytes before the clipboard is touched.
- Setting the default viewer (`viewer.setDefault`) changes the user's system settings and needs
  `--automation-allow-external` (M15.24, `crates/engine/tests/default_viewer.rs`). On Linux it
  merges `mimeapps.list` through `fs` (§4.2); on Windows it registers per user and opens the
  system's Default apps settings; on macOS it sets nothing (no safe binding to the role-handler
  API exists in the workspace) and says how to choose Dalica in Finder.
- Photos dragged out to another application (M15.23) hand over the files on disk, never the
  unsaved edits (a note says so on the drag). On X11 Dalica is its own XDND source over x11rb's
  pure-Rust connection (`drag_out::x11::tests` drops on a live XDND window of the test's own); on
  Windows and macOS the pinned `drag` crate keeps OLE's and AppKit's `unsafe` out of the
  workspace (§8.7). On Wayland dragging out is not built (the drag would have to start on winit's
  own connection, reachable only through raw pointers): the status line points at Copy, which
  puts the files on the clipboard. No target application was tried by hand on any OS (checked at
  M15.R).
- URLs Dalica opens are fixed `https://` addresses from its own configuration (map attribution,
  help), never URLs found in metadata.
- No external video process exists: reserved decision 2 chose pure-Rust decoders inside the
  process (M15.46, M15.15), parsed with the limits of §4.2.

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
| MCP over stdio, headless engine | `dalica-cli mcp [--demo]` | inherited from the process pipe | M10 |
| MCP bridge to the running app | `dalica-cli mcp --connect 127.0.0.1:<port>` | token (§6.3) | M10 |
| Files handed to the running Dalica (one Dalica per user) | every launch of the per-user library | token in `instance.json` | M15.18 |

Prefer stdio. Start the TCP control channel only when needed.

**The hand-off channel (M15.18, implemented:** `apps/dalica/src/instance.rs`, `instance/`,
`apps/dalica/tests/instance.rs`**).** A second launch of Dalica on the per-user library hands its
files to the running one instead of opening the library twice. The running Dalica holds
`instance.lock` (an exclusive OS lock, released when the process ends) and listens on
`127.0.0.1` at a port the OS chooses; `instance.json` (`port`, `token`, `pid`) is written through
`dalica_fs::replace_private` in the per-user settings folder (`0600` on Unix, the folder's ACL on
Windows). The token is 256 bits from the OS random source, compared in constant time, never
logged. A connection carries one request line within the limits of §4.2 (5 s in all, ≤ 64
absolute paths); any violation refuses the whole request and nothing opens; at most 8 connections
are served at once, and past that a connection is dropped and the sender retries. The receiver
answers `ok` only once its window has taken the paths (within 8 s); otherwise the sender starts on
its own after 10 s, so a file is never acknowledged and then dropped. A handed path can only open
the viewer on it (`Session::hand_viewer`), exactly what `dalica --view` does; nothing is written.
Launches with `--library`, `--demo`, `DALICA_HOME` or a portable copy never take part. The request
line has no fuzz target (§7.1).

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
  client sends anything else. **Planned (M10).**

Bearer-token authentication with the token file: **Planned (M2).**

### 6.4 Only JSON request lines are read

Every line must be a UTF-8 JSON **object** with a string `method` (`id` and `params` optional;
blank lines are skipped). Anything else (text that is not JSON, a JSON array or number, an object
without `method`, invalid UTF-8, a line over the size limit, or one nesting arrays and objects deeper than 64 levels) gets one error reply
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
| Batch requests (control or MCP) | ≤ 256 steps; an aggregate reply budget stops later steps when exhausted | M10 |
| Query results (photo lists) | paged, ≤ 10,000 items per page | M10 |
| Rendered previews returned to agents | ≤ 2,048 px edge by default; encoded image ≤ 5 MiB | M10 |
| MCP tool arguments / results | arguments ≤ 1 MiB; encoded results ≤ 8 MiB | M10 |

A reply can be rejected after a command has already changed state; such errors say that the
operation may have completed, and earlier batch steps are not rolled back. Inspect state before
retrying. A bounded reply does not imply a bounded command cost.

### 6.6 Command policy and file-system authority

- **Ids, not paths.** Commands address photos, folders, albums and people by library id. An id can
  only reach files already indexed under a watched folder, so most commands need no path
  authority at all.
- **Path-bearing commands** (add a watched folder, export destination, Save As, import source,
  a face model file to inspect or install, collage destination, `ui.screenshot {path}`,
  render-to-file) accept a path from automation only
  if it resolves beneath a root granted at launch (`control-protocol.md` §5.7, `mcp.md` §5). One
  policy serves every surface: `dalica-engine`'s `automation` module, which the control channel
  and the MCP server call (built M10.16, after PhotoCraft's `AuthorizedWorkspace`):
  - **read roots** (folders that may be added or read): `--automation-read-root <dir>`
    (repeatable), the folders given to `dalica-cli mcp` on its command line, the demo's folder,
    and an MCP client's `file://` roots (§6.7); a watched folder may always be read again; with
    no read root at all, adding a folder fails closed, except in `--demo`;
  - **write roots** (where new files are made): `--automation-write-root <dir>` (repeatable); with
    none given, the system temporary directory and `<cache>/Dalica/automation/`. Client roots are
    never write roots;
  - never inside `.dalica-originals/` (in any letter case); a screenshot or render never inside a
    watched folder (it would become a library photo); an export folder and a Save As file may be,
    since they join the library by design;
  - never over an existing file, except that a screenshot or render may replace a `.png` (a link
    there must end at a `.png` file); exports never overwrite (`-001` names);
  - effects outside Dalica are refused without `--automation-allow-external` (§5.11); people
    names and face regions are withheld without `--automation-allow-people`.
- **Path rules.** A path must be absolute and at most 4096 bytes; a `..` component is refused
  before anything is resolved, and so is a `.`. On Windows a verbatim or device prefix (`\\?\`,
  `\\.\`), a `:` inside a component (a stream), a component ending in a dot or a space and a
  reserved device name (`NUL`, `COM1.txt`) are refused too, before any I/O. Then the path (for a
  new file, its folder; for an export folder that does not exist yet, its nearest existing
  ancestor) is resolved through its links and compared against each resolved root by **path
  components** (a string prefix would accept `/photos-evil` for `/photos`); a root that does not
  exist holds nothing. Comparisons that allow are exact; those that refuse (a watched folder,
  `.dalica-originals`) also ignore letter case on Windows and macOS, whose volumes usually do. A
  new file's folder must already exist; only an export creates its single output folder.
  Checks resolve links, so they run off the UI thread; the lexical part runs first, on it.
- **Refused over automation:** commands that open a native file dialog (supply the path parameter
  instead), launch external programs (§5.11; with deleting from a camera or card, the commands of
  `automation::NEVER_AUTOMATED`, refused whatever was granted), permanently delete card contents (§5.7), change the
  watched-folder list or other file-bearing preferences except through the root-checked commands
  above, or download anything (models, tiles). Individual safe preferences remain settable.
- **Same policy for every route.** Synthetic UI input (`ui.clickWidget`, `ui.key`, `ui.type`,
  `ui.menu.invoke`) goes through the same policy as `engine.execute`, and so does every command a
  command runs on its own behalf (batch edit, actions, import steps), so an allowed command cannot
  reach a refused one.
- **Destructive commands stay safe by construction:** delete goes to the trash, Save keeps the
  original in `.dalica-originals/`, metadata write-back merges. Automation has no way around these.
- **Read-only sessions.** `dalica-cli mcp --read-only` opens the engine session read-only
  (`Session::open_read_only`): the refusal lives at two seams of the engine's session and nowhere
  else, so no tool or route can step around it. Commands that write files (saves, exports, file
  operations: `Effect::Files`) answer `read_only`; every other write (the index journal and
  snapshots, sidecars, `folders.json`, `settings.json`, `albums.json`, metadata write-back into
  photo files) stays in memory and is gone when the session closes; the repairs at open
  (interrupted saves and file operations) wait for a session that may write. What still touches
  the disk: the library's lock file, the thumbnail cache (outside the photo folders), a damaged
  index set aside at open as in any session, and a scan's removal of Dalica's own stale temporary
  files (`.<name>.dalica-tmp-<random>` over 24 hours old, §5.8). Use it, or `--demo`, for agents
  you do not trust. A read-only mode for the desktop app's control channel is not built. Options
  changed through `options.set` are refused in a read-only session (their write is answered
  first), with the message that they could not be saved.

Command policy and roots: built for every path-bearing command that exists (M2 screenshots, M8
exports and Save As, M10.16 the shared engine policy, client roots and the `--automation-*`
flags of `dalica-cli mcp`); collage and import destinations follow it as they land.

### 6.7 MCP specifics

- Headless `dalica-cli mcp` grants file-system authority with `--automation-read-root <dir>` and
  `--automation-write-root <dir>` (same semantics as §6.6; flags only, no environment variable
  widens an MCP server's roots). If the MCP client declares the `roots` capability, the server
  asks `roots/list` after `initialize` and again on each `notifications/roots/list_changed`; each
  local `file://` root (no host or `localhost`; another scheme, a remote host, a `..` or a bad
  escape is dropped) becomes a **read root only**, never a write root, and the latest list
  replaces the previous one, so a removed root stops granting at once. An error answer drops
  every client root; an answer to an older request is ignored. `--no-client-roots` never asks.
  The client's roots widen what may be read, as the folders a user lists on the command line do:
  a client is the user's own agent, and writing stays confined to the write roots.
- In bridge mode (`--connect`), roots and read-only mode are configured on the `dalica` process;
  the bridge cannot widen them, and the client's roots are ignored.
- Tools carry MCP annotations: read-only tools are marked read-only; delete, move, rename, Save,
  Revert and metadata-writing tools are marked destructive so clients can ask the user first.
- Tool descriptions and results treat captions, tags, file names and people names as untrusted
  user content (§4.4). Results never include tokens, face descriptors or absolute paths outside
  the granted roots.
- The bridge bounds incoming replies, drops the connection after a transport failure or timeout,
  reports that the operation may have completed, and never resends it automatically.

Built: the roots and `--read-only` (M10.16), the annotations (M10.15). Bridge mode: **Planned
(M10.17).**

### 6.8 Control status

| Control | Status |
|---|---|
| Loopback-only TCP binding | Implemented (M2; `control_security.rs`) |
| Token authentication from a `0600` token file | Implemented (M2; `control_security.rs`, `control_headless.rs`); the "owned by the current user" check of §6.3 is not yet built, and on Windows the file keeps its folder's ACL |
| JSON-request-only lines, close on junk | Implemented (M2; `control_security.rs`) |
| Request, reply, depth, connection, timeout limits | Implemented (M2; `control_security.rs`, the `ui` control tests; depth 64 since #102) |
| Screenshot / snapshot dimension limits | Implemented (M1, M2; the `ui` control tests) |
| Command error contract and `panic_hunt` | Implemented (M1; `panic_hunt`) |
| Root-checked path parameters | Implemented (M2 screenshots, M8 exports and Save As; one engine policy since M10.16: `crates/engine/tests/automation_roots.rs`, the `ui` control tests); collage destinations over the control channel, the default folder included (M14.17: `crates/ui/src/control/engine.rs`; the MCP server's with M14.21); import destinations as they land; the video outputs (M15.25, M15.26, M15.28): `video.exportClip` and `player.exportClip` take their `path` as a new file inside a write root (`WRITE_PATHS` in `crates/ui/src/control/engine.rs`, `check_paths` in `crates/mcp/src/headless.rs`), Save Frame as Picture writes into the Video Frames folder only when it lies inside a write root (a control request's click outside is refused and writes nothing: `crates/ui/tests/video_player.rs` `a_control_click_on_save_frame_outside_the_write_roots_is_refused_and_writes_nothing`; headless MCP: `crates/mcp/src/headless.rs`'s tests), and Export's videos (`export.run {videos}`) and a collage's poster frames go to the destinations already checked |
| Commands never automated | Implemented (M15.17: `automation::NEVER_AUTOMATED`: `import.removeFromSource`, `photo.locate`, `folder.locate`, `photo.openWith`, `photo.openExternal`, refused with `external_denied` whatever was granted, each for its own reason since M15.42; M15.22: `App::run` refuses them while a control request drives the window) |
| People data withheld | Implemented (M13; `--automation-allow-people`): names, nicknames and face rectangles (`PEOPLE_REPLIES`, `FACE_POINTS`, `NAME_SEARCHES`); a person's export folder (M14.25, M15.42, M15.51: `withholdNames` on `library.query`, `search.suggest` and `slideshow.list`; the job, its folder, its failed items' reasons, its toast and the edit view's status prefix withheld in `ui.inspect`, `ui.state`, `jobs.*` and headless MCP; `crates/ui/src/control/tests.rs`, `crates/engine/tests/search.rs`, `crates/mcp/tests/command_tools.rs`). Accepted limit (maintainer decision at M15.R, 2026-10-20): a folder's own name is user-chosen data and still shows inside photo paths (`library.query`, `photo.inspect`, `library.inspect`), so a person's export folder named after them shows that name there |
| Batch limits, paging, preview limits | Planned (M10) |
| MCP roots, read-only mode, tool annotations | Implemented (M10.15 annotations; M10.16 client roots, `--automation-*`, `--read-only`: `crates/mcp/tests/roots.rs`, `crates/engine/tests/read_only.rs`); read-only for the desktop app's control channel not built |
| Server proof in the handshake | Planned (M10) |
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
| `crates/image` | `sniff`, `decode_any`, `decode_jpeg`, `decode_png`, `decode_tiff`, `decode_webp`, `decode_gif`, `decode_bmp`, `decode_jxl`, `decode_psd`, `decode_heif` (each format decoded explicitly, no sniffing), `raw_preview` (every RAW container's recognition and preview decode), `embedded_thumbnail` (EXIF thumbnails, thumbnail IFDs, PSD resources, RAW previews), `icc`, `video_probe` (any bytes as a video's header: refused or truncated, or facts in range, the box and EBML limits of §4.2 holding) | **Implemented** (M5.1, in `crates/image/fuzz/`; `video_probe` M15.2: 7.6 M runs in 181 s before merging, one finding (an 8-byte EBML size overflowing a shift) fixed with its regression test; the targets reach each decoder as M5.6–M5.10 filled it; `decode_heif` reaches `heic-rs` only when built with `--features dalica-image/heif`, and the Fuzz workflow builds without it, so there it fuzzes our box walk); built and run for 60 s each at every milestone review and when the Fuzz workflow is dispatched |
| `crates/meta` | `exif` (IFDs and maker notes), `xmp` (including extended-XMP reassembly), `iptc`, `write_merge` (parse → merge-write → re-parse: no panic, foreign fields unchanged), `carry` (any file as the source of a rendered copy's metadata: an error, or a JPEG or PNG whose metadata reads back, orientation 1, the size asked, no GPS when the location was removed; setting the orientation reads back as asked), `regions` (MWG face regions: an XMP packet read, then written into with named faces; a merge that succeeds reads back exactly the faces written, writing them again changes nothing, and the same packet in a JPEG passes `write_owned`'s verification) | **Implemented** (M6.4, in `crates/meta/fuzz/`; `carry` M8.7; `regions` M13.11; `write_merge` reaches every container writer (JPEG M6.17, PNG, WebP and TIFF M6.18) and asserts that a returned write passes its verification, reads back and writes again to the same bytes; `xmp` reaches the XML reader (M6.9), the merge writer and extended-XMP reassembly with MD5 GUIDs, and writes its JPEG through `write_owned` (M6.17)); built and run for 60 s each at every milestone review and when the Fuzz workflow is dispatched. Local runs before merging (stable with cargo-fuzz's flags): `write_merge` 2.8 M runs in 180 s and `xmp` 0.8 M in 60 s with no finding after the one idempotence finding M6.17 fixed (`tests/write_jpeg.rs` keeps its regression test). The metadata corpus (M6.34, `crates/meta/tests/corpus.rs`) runs the same write rules over files other applications wrote |
| `crates/faces` | `model` (any bytes as a face model: refused with a reason, or loaded and run on a fixed input of its declared shape, each dynamic size taken as 1, giving finite outputs of the shapes promised by the input check) | **Implemented** (M13.6, in `crates/faces/fuzz/`, seeded by `dalica_faces::synthetic`); built and run for 60 s at every milestone review and when the Fuzz workflow is dispatched. Local run before merging (stable with cargo-fuzz's flags): 7.0 M runs in 180 s from the synthetic models, 1,235 corpus entries, no finding |
| `crates/fetch` | `response` (any bytes as a server's reply, read as the HTTPS transport reads one after the TLS layer, the first byte choosing how many bytes each read hands out: an error, or a status of 200–599 and a body no longer than the input and exactly its announced length; a `Location` it carries leads to an https address or is refused) | **Implemented** (M13.13, in `crates/fetch/fuzz/`); built and run for 60 s at every milestone review and when the Fuzz workflow is dispatched. Local run before merging (stable with cargo-fuzz's flags): 3.3 M runs in 180 s from four seed replies, 794 corpus entries, no finding |
| `crates/sidecar` | `parse` (any bytes as a folder's `.dalica.json`, an originals record and `albums.json`, under the default and small limits: refused by a named limit or as not JSON, or every typed read returns, every face rectangle inside the picture with its corners in order, every geotag on the globe, every usable originals entry under a plain name), `roundtrip` (print → parse gives the same document and printing is stable, for all three; writing back every value read reads back the same and a second write-back changes nothing; every key Dalica does not read, at the top level, in `"folder"` and in each photo's entry, keeps its value), `recipe` (every `edits`, `redo` and saved or original `recipe` list a sidecar or originals record holds, and the input's own lines as one list, on a picture size and EXIF orientation from the first bytes: the list prints back exactly, each operation prints its own text, typed parameters, clamped or not, print to text that reads back as the same parameters, and the pipeline, orientation and shown size return), `legacy` (any bytes as a legacy sidecar under the default and small limits: refused by a named limit, or every typed read returns, every listed file is found, every rectangle inside the picture with its corners in order, every contact id and album token of its fixed shape) | **Implemented** (`legacy` M14.2; `parse`, `roundtrip` and `recipe` M15.39; in `crates/sidecar/fuzz/`, each started from its `seeds/<target>`); built and run for 60 s each at every milestone review and when the Fuzz workflow is dispatched. Local runs before merging (stable with cargo-fuzz's flags, 180 s each): `legacy` 4.9 M runs, 2,333 corpus entries; `parse` 5.4 M runs, 5,251 entries; `roundtrip` 5.2 M runs, 5,168 entries; `recipe` 2.3 M runs, 7,428 entries; no finding. `crates/sidecar/src/tests.rs` keeps its property tests (random bytes never panic, print → parse is the identity, any file name round-trips as a key) |
| `crates/store` | `journal` (any bytes as `index.log`, as they are or with each line framed as a checksummed record, its last newline kept or torn: reading stops at the first torn, mismatched or unreadable record with every record before it whole, the valid prefix reads again the same; the ops replay onto an empty model as opening the store does, and the model they build, written as a snapshot, loads back and writes the same bytes again), `snapshot` (any bytes as `index.snap`, as they are or framed as one record: refused with a reason, or loaded, and the loaded model written, loaded and written again gives the same bytes at the same op number), `people` (any bytes as `people.json`: refused as damaged, or print → parse gives the same document and printing is stable; writing back the people, legacy ids and rejected suggestions read reads back the same, keeps every top-level key Dalica does not know, and a second write-back changes nothing) | **Implemented** (M15.39, in `crates/store/fuzz/`, each started from its `seeds/<target>`); built and run for 60 s each at every milestone review and when the Fuzz workflow is dispatched. Local runs before merging (stable with cargo-fuzz's flags, 180 s each): `journal` 3.9 M runs, 6,326 corpus entries; `snapshot` 6.5 M runs, 8,497 entries; `people` 5.7 M runs, 4,580 entries; no finding. `albums.json` is the sidecar crate's (`parse`, `roundtrip`); `folders.json` and `settings.json` are the engine's (`folders_json`, `settings_json`). The journal's tests still read every prefix of it in memory and reopen the store at each record boundary (`crates/store/src/lib.rs`), and tear the sketch file's tail (`crates/store/tests/sketch.rs`) |
| `crates/ui` control | `request_line` (any bytes as one line after `auth`: refused with `parse` (over the line limit, not UTF-8, nested past §6.5's depth, not JSON, not an object, no string `method`), skipped when blank, or a request whose parameters nest within the limit, handled by the headless app with no library open, a stub file picker and the default automation settings: an answer given up front is never `internal`, input it would play and work it would run are dropped unplayed so nothing reaches the disk; every answer is one JSON line under the reply limit echoing the id) | **Implemented** (M15.39, in `crates/ui/fuzz/`, started from `seeds/request_line`); built and run for 60 s at every milestone review and when the Fuzz workflow is dispatched, with `--cfg fuzzing` like every other target (the fuzz crate builds `dalica-ui` without its `camera-usb` feature, so `nusb` is not in its graph; M15.52). Local run before merging (stable with cargo-fuzz's flags): 2.4 M runs in 180 s, 3,521 corpus entries, peak 113 MB, no finding. The `auth` step before it is the app's transport's (`apps/dalica`, `auth`) |
| `crates/scan` | `ptp` (what a camera sends to the PTP/MTP reader: random containers cut into random transfers through a whole session and each dataset parser; every answer an error or values within §4.2's limits) | **Implemented** (M15.9, in `crates/scan/fuzz/`; the fuzz crate builds `dalica-scan` without its `usb` feature, since `nusb` does not build under `--cfg fuzzing`); built and run for 60 s at every milestone review and when the Fuzz workflow is dispatched. Local run before merging (stable with cargo-fuzz's flags): 5.7 M runs in 181 s, 976 new units, peak 48 MB, no finding. `crates/scan/tests/ptp.rs` keeps its property test (random containers never panic) |
| `crates/engine` | `folders_json` (any bytes as the library's `folders.json`, through the parser a session opens it with: refused (not JSON, not an object with a `folders` array, a version that is not a whole number), or every folder read is one absolute path in its normal form, read once, with one rule that sets something (only Scan Once marked indexed), the units are the folders the rules index, every entry is read or kept as it was, every unknown top-level key keeps its value; the file written back (v2) reads again as the same folders, rules, unknown keys and kept entries, and writes the same bytes again), `settings_json` (any bytes as `settings.json`: an error with its reason, or settings in which every option of the Options catalogue reads a value its own check accepts unchanged (a stored language is any language tag), the window, tree width, thumbnail size and scrolling curve stay in their ranges; they print, parse back equal and print the same bytes; every value stored back through its option's `store` and through each typed setter reads back the same, and a second store changes nothing) | **Implemented** (M15.53, in `crates/engine/fuzz/`, each started from its `seeds/<target>`; the fuzz crate builds the engine without `camera-usb`; `folders_json` reaches `Folders::parse` through a `#[doc(hidden)]` entry point, `folders::parse_for_fuzzing`); built and run for 60 s each at every milestone review and when the Fuzz workflow is dispatched. Local runs before merging (stable with cargo-fuzz's flags, 180 s each): `folders_json` 1.8 M runs, 3,864 corpus entries; `settings_json` 1.4 M runs, 3,850 entries; no finding. M15.28's squash merge (#1090) removed every file of M15.53 (this row, the `auth` row below, their `fuzz.yml` jobs, the root manifest's excludes); the 1.0 audit (M15.36) found it and an M15.R fix put them back |
| `apps/dalica` control | `auth` (any bytes as what a client sends first on a connection, against a fixed token, through the transport's own line framing (`frame`, each read handing out as many bytes as the input says, under the transport's line limit or a small one) and the step each line takes (`step`): lines are cut at each newline whatever the reads hand out, and one over the limit is refused with `parse` and nothing after it is read; a blank line is skipped, a line that is not a request is answered `parse` and closes; until a successful `auth` nothing is passed on, `auth` succeeds exactly when its token is the token (compared in constant time) and anything else is answered `unauthorized` and closes; after it `auth` is still answered by the transport and every other request is passed on; every answer is one JSON line echoing the id, never showing the token) | **Implemented** (M15.53, in `apps/dalica/fuzz/`, started from `seeds/auth`; the fuzz crate compiles `src/control/transport.rs` and `token.rs` in with `#[path]`, so the window, GPU and camera dependencies stay out of its graph); built and run for 60 s at every milestone review and when the Fuzz workflow is dispatched. Local run before merging (stable with cargo-fuzz's flags): 1.2 M runs in 180 s, 1,956 corpus entries, peak 91 MB, no finding. The transport's tests still cover the socket side: the accept limit, the auth deadline and the idle timeout |
| `crates/mcp` | `tool_args` | Not built (planned at M10, never scheduled): the MCP server's arguments are fuzzed only through `panic_hunt`'s hostile parameters to the commands they map to |
| `crates/image` video and sound | `video_frames` (a clip's frames through `FrameSource`, as the player, the slideshow and the posters read them: the MP4 sample tables and Matroska clusters, the gate on every SPS, `altc264` and `rusty_h265` (the fuzz crate builds `dalica-image` with `hevc`), the conversion to upright sRGB; the first byte chooses a whole file, or parameter sets and samples the target muxes into an MP4 so every run reaches a decoder, with one or two threads: refused with a reason, or at least one frame, and every frame (poster, steps both ways, the frame at a time) at most the size asked for, whole, numbered and timed within the clip), `video_sound` (a clip's sound through `SoundSource`: the first sound track of MP4 or Matroska, laced or not, its `AudioSpecificConfig` and `rusty_aac`; a whole file, or a configuration and packets the target muxes: muted with a reason, or a rate and channels within the limits and every chunk whole frames of finite samples timed within the track, before and after a seek), `music` (any bytes as slideshow music through `Music`: MP3, Ogg Vorbis, FLAC, WAV or refused; it opens as the kind it sniffs as, at a rate and channels within the limits, every block whole frames of samples in −1 ..= 1, a failure said once and the file ended) | **Implemented** (M15.R, in `crates/image/fuzz/`; the gate on the decoders' `unsafe`, §7.2, §8.7). Their seeds are written procedurally into `corpus/` by the crate's `seeds` example from the test suite's own writers (`tests/clip_writer.rs`, `tests/music_writer.rs`, compiled in with `#[path]`) and an HEVC VPS, SPS, PPS and IDR slice header written there; no media file is committed. A decoder's panic that Dalica's `catch_unwind` catches is let through and named once, any other panic is a finding. Local runs before merging (stable with cargo-fuzz's flags, `ulimit -v 8000000`, 60 s each): `video_frames` 89 k runs (1,458/s), coverage 7,045, peak 84 MB, no finding (three of `altc264`'s overflow checks in `transform.rs`, caught by the guard: hostile coefficients overflowing `i32`, which a release build wraps into wrong pixels); `video_sound` 147 k runs (2,411/s), coverage 3,028, peak 53 MB, no finding; `music` found two: an Ogg Vorbis comment header declaring 2³² − 1 comments made `lewton` reserve 205 GB and abort the process (a codebook's entries times dimensions does the same), now refused first (`formats/audio/vorbis.rs` reads the three header packets through `lewton`'s `ogg` reader and checks every declared size), and damaged Vorbis audio decoding to NaN, now made finite and clamped to −1 ..= 1; both with regression tests in `tests/audio.rs`; after them 957 k runs (15,685/s), coverage 2,714, peak 53 MB, no finding (one `lewton` panic caught by the guard) |
| `apps/dalica` hand-off | `handoff` (any bytes as the line a later launch sends the running Dalica, through `instance/wire.rs`'s `parse_request` against a fixed token read back through the info file as a later launch reads it; the first byte `!` puts the token in so lines reach past its check: refused with a reason, or the token, at most 64 absolute paths of at most 4,096 bytes, a line over the limit never read; a request the sender makes of those paths reads back as the same paths and wait, and is refused under another token; the same bytes as a reply are taken exactly when `ok` is `true`) | **Implemented** (M15.R, in `apps/dalica/fuzz/`, started from `seeds/handoff`; `wire.rs` compiled in with `#[path]`); built and run for 60 s at every milestone review and when the Fuzz workflow is dispatched. Local run before merging (stable with cargo-fuzz's flags): 8.0 M runs in 61 s (130 k/s), coverage 1,106, peak 32 MB, no finding. The line's framing is the control transport's, fuzzed by `auth` |
| system handler lists | (`apps/dalica/src/os/`'s readers) | Not built as a fuzz target; `apps/dalica/tests/os.rs` runs 2,000 random inputs through every parser (20,000 at full strength) |

Command parameters are fuzzed by `panic_hunt` (M1, an ordinary `dalica-engine` test), which runs
every registered command with hostile parameters generated from its spec and must stay green.

### 7.2 Running

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
cd crates/meta
cargo +nightly fuzz run xmp -- -max_total_time=300
```

Reproduce a crash, minimize it (`cargo fuzz tmin`), add a deterministic regression test that fails,
then fix it. A time-limited run with no findings is evidence of that run only, not proof that a
parser is secure.

CI: the Fuzz workflow (`.github/workflows/fuzz.yml`, M5.1, M6.4, M13.6, M13.13, M14.2, M15.9,
M15.39, M15.53, M15.R) builds every `image`, `meta`, `faces`, `fetch`, `sidecar`, `scan`, `store`, `ui`,
`engine` and `dalica` target (one job per crate) and runs each for 60 s on the pull request of every
milestone review (maintainer decision, 2026-10-15; before that the targets were built but never
run), and for a chosen time when dispatched by hand. A crash fails the review's run and keeps its
input as an artifact; the fix is a task merged before the review closes. Fuzzing runs at reviews
only (maintainer answer to M15.R's Q14, 2026-10-20): there is no scheduled weekly run, as GitHub
runs nothing between reviews. Continuous hosted fuzzing is future work.

The decoders' gate (maintainer answer to M15.R's Q2, 2026-10-20): the `unsafe` code in `altc264`,
`rusty_h265` (with `simd`) and `rusty_aac` is accepted because `video_frames` and `video_sound`
reach it through Dalica's own entry points at every review, 60 s each, under the review's
AddressSanitizer build (cargo-fuzz's default), and a finding there blocks the review like any
other. The image job writes their seeds first (`cargo +nightly run --release --example seeds`).

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
inputs and the few small starting inputs a fuzz workspace keeps in `seeds/` (text written for
the purpose or by the crate's own writer, never personal files).

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

Map tiles, geocoding and model downloads (M13) need TLS. The usual Rust TLS providers (`ring`,
`aws-lc-rs`) contain C or assembly. The choice was reserved decision 9 in `roadmap.md`.

**Decided 2026-10-18 (the maintainer, on #814): pure Rust.** The HTTPS transport of `dalica-fetch`
(M13.13, ported from LightCraft's) is:

- `rustls` 0.23 with its default features off (`std` and `tls12` only), so its `aws-lc-rs`
  provider is never built; TLS 1.2 and 1.3 with rustls's safe defaults;
- `rustls-rustcrypto` as the crypto provider: RustCrypto's AES-GCM, ChaCha20-Poly1305, X25519,
  P-256/P-384 ECDSA, Ed25519 and RSA signatures in pure Rust, no `ring`, no assembly, no C build;
- `webpki-roots` 1: Mozilla's root certificates compiled in, so validation never depends on the
  system's store. Its licence, `CDLA-Permissive-2.0` (a permissive licence for data: the
  certificates are data, and sharing them asks only that the licence text go with them), is not in
  §8.3's list; it is the one reviewed `LICENCE_EXCEPTIONS` row in `xtask/src/deps.rs`, allowed by
  the maintainer with this decision. Its licence text must travel with the release packages from
  the first release that links `dalica-fetch` into the app: the engine depends on it from M13.16
  and M13.20 (face model downloads, map tiles and place searches), so 0.8 is that release; the
  text is in `NOTICE` (*Data compiled into the program*), which every package ships.

What was weighed: `rustls-rustcrypto` is an **alpha** release (0.0.2-alpha), younger and far less
widely deployed than `ring` or `aws-lc-rs`. It is accepted because the alternatives break §8.1 (`ring` and `aws-lc-rs`
build C and assembly) or leave the user's network to each platform's downloader, and because what
the transport protects is small: downloads are pinned to a SHA-256 in the source, and map tiles
are public pictures. Moving to a stable pure-Rust provider when one exists is a dependency update,
not a redesign (the provider is one line of `http/wire.rs`).

`Cargo.lock` lists `ring` as an optional dependency of `rustls-webpki` that no enabled feature
turns on; it is never built (`cargo tree -i ring` prints nothing, and `cargo xtask deps` checks the
resolved graph, §8.1). `cargo metadata` still lists it under `rustls-webpki`, named by the weak
feature `alloc = ["ring?/alloc"]`; since M13.20 (when the engine began to link `dalica-fetch`) the
gate counts an optional dependency only when the parent's resolved features turn it on (`ring`,
`dep:ring`, `ring/…`, through any chain), and a test keeps it agreeing with `cargo tree -i ring`
on every target. `rustls-rustcrypto` also brings an older `rustls-webpki` (0.102) for its
signature-algorithm types; the certificate chain is checked by rustls's own `rustls-webpki` 0.103.

Whatever the stack: certificate validation is never disabled (there is no setting or API to turn
it off; a test root is trusted only by a transport built for it with `HttpsTransport::with_roots`,
which replaces the bundled roots), downloaded models are checked against a SHA-256 pinned in the
source, response sizes are capped (§4.2), redirects lead only to https addresses, and requests use
a generic User-Agent with no personal details and no cookie, token or credential. A proxy is used
only through `CONNECT`, so it sees the host and port, never the request or the file, and the
certificate is checked against the server's own name.

### 8.3 Licences

Allowed for dependencies: `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`,
`BSD-3-Clause`, `ISC`, `Zlib`, `BSL-1.0`, `Unicode-3.0`, `Unicode-DFS-2016`, `CC0-1.0`, `MIT-0`.
`MPL-2.0` only as a reviewed per-crate exception for unmodified dependencies. Reviewed per-crate
exceptions (`LICENCE_EXCEPTIONS` in `xtask/src/deps.rs`, each the maintainer's): `webpki-roots`
(`CDLA-Permissive-2.0`, the bundled root certificates; §8.2, 2026-10-18). Never `GPL`, `LGPL`,
`AGPL`, `SSPL` or unlicensed crates (`process.md` §3). Asset licences are a separate check
(`cargo xtask assets`).

### 8.4 Advisories, sources and duplicates

- RustSec advisories against the crates the release packages ship (the product graph of every
  release target, as the SBOM of §8.5 lists it) fail `cargo xtask advisories`. An exception is
  recorded below, one bullet per advisory id, with an owner, a rationale, a compensating control
  and a review date (`*Review:*` with a `YYYY-MM-DD` date); the check reads these bullets, and an
  entry missing a field fails it. A recorded advisory whose review date has passed, and a record
  no shipped crate matches any more, are warnings. Informational advisories (unmaintained,
  unsound, notices) are printed as warnings and reviewed at each milestone review, not ignored.
- Recorded advisories:
  - **RUSTSEC-2023-0071, `rsa` 0.9** (the Marvin attack: a timing side channel in RSA
    *decryption* that can leak the private key to someone who measures many decryptions). Brought
    in by `rustls-rustcrypto` (§8.2). *Rationale:* Dalica is only ever a TLS client and never holds
    an RSA private key; the client uses `rsa` only to *verify* servers' RSA signatures, which are
    public-key operations with no secret to leak (rustls has no RSA key exchange at all, and Dalica
    sends no client certificate). *Compensating control:*
    downloads are pinned to a SHA-256 in the source, so a broken channel cannot change a model.
    *Owner:* the lead. *Review:* by 2027-01-31, at every milestone review, and when `rsa` 0.10
    (constant-time) reaches `rustls-rustcrypto`.
  - **RUSTSEC-2026-0049, RUSTSEC-2026-0098, RUSTSEC-2026-0099, RUSTSEC-2026-0104,
    `rustls-webpki` 0.102** (in certificate path validation: CRL distribution points not matched,
    a reachable panic in CRL parsing, and two name-constraint checks that accepted what they should
    refuse). Brought in by `rustls-rustcrypto` 0.0.2-alpha (§8.2), which uses its 0.102 copy only
    for the `alg_id` constants of its signature algorithms (`src/verify/{rsa,ecdsa,eddsa}.rs`).
    *Rationale:* no code of the 0.102 copy validates a certificate or parses a CRL: chains are
    validated by rustls's own `rustls-webpki` 0.103 (0.103.15 at M15.40, which has all four
    fixes, and `cargo xtask advisories` keeps checking it), and Dalica configures no CRLs.
    *Compensating control:* downloads are pinned to a SHA-256 in the source, so a broken channel
    cannot change a model. *Owner:* the lead. *Review:* by 2027-01-31, at every milestone review,
    and when `rustls-rustcrypto` moves to `rustls-webpki` 0.103. Recorded at M15.40 (2026-10-20)
    when the check first ran; the maintainer confirms it at M15.R.
- Sources: crates.io, plus git dependencies on the LightCraft and PhotoCraft repositories pinned to
  a full commit hash (never a branch). Other git sources are refused.
- Duplicate versions warn; duplicates of parser crates are reviewed.

### 8.5 Checks

```sh
cargo xtask deps                  # offline: licences, banned and *-sys crates, C builds, sources
cargo xtask advisories [--fetch]  # RustSec advisories against the shipped crates (§8.4)
cargo xtask sbom [--out <dir>]    # a CycloneDX SBOM per release package
```

`xtask deps` runs inside every `cargo xtask ci`. **Implemented (M15.40):** `xtask advisories` and
`xtask sbom`, both in `xtask` itself (pure Rust, no new dependency, no C), never linked into the
product:

- **Advisories.** The check matches the RustSec advisory database (a checkout of
  `github.com/rustsec/advisory-db`: `--db <dir>`, `DALICA_ADVISORY_DB`, or `target/advisory-db`)
  against the crates the release packages ship, with Cargo's semver rules for the `patched` and
  `unaffected` ranges and each advisory's `os`/`arch`; withdrawn advisories are skipped. It reads
  the database from disk and is offline; only `--fetch` uses the network, to clone or update the
  checkout. It runs once per milestone review, as a step of `cargo xtask ci --full` (with the
  static checks, so only the Linux run of the review has it, never once per OS; it fetches unless
  `DALICA_ADVISORY_DB` names a checkout), and in every run of the release workflow, dry runs
  included, before the checksums: an unrecorded vulnerability stops the release. Its tests run
  offline on a fixture database. `cargo deny` and `cargo audit` were weighed and not used: either
  is one more tool to install and pin in CI, neither reads its exceptions from this document, and
  the matching is small; `deny.toml` does not exist.
- **SBOM.** One CycloneDX 1.5 JSON document per release package (`dalica-<v>-linux-x86_64`,
  `linux-aarch64`, `windows-x64`, `macos-universal`, each `.cdx.json`), attached to the GitHub
  release next to `SHA256SUMS.txt` and listed in it. Its components are the product graph of
  `xtask deps` for each target the package is built for (the macOS package is the union of its two
  targets, and each component names its targets), kept to what `cargo tree` says the release build
  (`-p dalica -p dalica-cli`, default features) compiles, since `cargo metadata` unifies features
  across the workspace; a target-gated crate (`nusb`, Linux and Windows) is listed only where it
  builds, and an off-by-default feature (`hevc`, `heif`) adds nothing. Each component carries its
  licence expression, its purl, the SHA-256 of its `.crate` from `Cargo.lock` and its dependency
  edges. The output is sorted and has no random serial number; its timestamp is the release's build
  date. `--list` prints every crate with its licence per target.

CI tools and GitHub Actions are dependencies too: actions are pinned to a full commit hash and
reviewed. The advisory database is the one input fetched unpinned, on purpose: an advisory published
yesterday must fail today's release; the run prints the database's revision.

### 8.6 Adding a dependency

The commit body says why the crate is needed, whether it processes untrusted input, its `unsafe`
and native-code exposure, `build.rs` and proc-macro behaviour (no network access at build time),
supported targets, maintenance status and licence. Reused Craft-app code (`architecture.md` §9)
and any other vendored code keep a header naming their origin and get a `NOTICE` row. A scanner
result does not replace reading the code. Community batches follow the checklist in `process.md`
§4 (new dependencies, `build.rs` or CI changes, `unsafe`, hidden Unicode, network access, file
writes outside the library).

### 8.7 Dependencies M15 added

Recorded by the 1.0 audit (M15.36) from each task's pull request; every one passed `cargo xtask
deps` (pure Rust, no C build, licences in §8.3) when it landed. `unsafe` counts are the crates' own
(M15.46's record, `video-decoders.md` §7, for the decoders; a count of the keyword in the crate's
`src` for the rest, which overstates: comments and `forbid` attributes count too). Dalica's own code
stays `#![forbid(unsafe_code)]`; the crates below are where M15 accepted `unsafe` to keep it out of
the workspace.

| Crate (pin) | Licence | Brought by | Reaches | `unsafe` | Note |
|---|---|---|---|---|---|
| `altc264` =0.1.1 | MIT | M15.15 (H.264 frames, every build) | hostile video bytes | 257 sites in 15 files (SIMD intrinsics, raw-pointer picture slices, `get_unchecked`, a lifetime `transmute` in its thread pool) | first released 2026-10-14, one maintainer; the coded size is checked before it sees a stream and every call is panic-guarded (§4.2). **Accepted behind a fuzz gate** (maintainer answer to M15.R's Q2, 2026-10-20): `video_frames` reaches it through `FrameSource` at every review, 60 s under AddressSanitizer (§7.1, §7.2); a finding blocks the review |
| `rusty_h265` =0.6.2 with `simd` | Apache-2.0 | M15.15 (HEVC frames) | hostile video bytes | 0 in the decoder (`forbid`); 702 in `rusty_h265-accel`'s SIMD kernels | only with `dalica-image`'s non-default `hevc` feature, which the apps do not forward: **not in the shipped build**. Its `unsafe` is accepted behind the same fuzz gate (M15.R Q2): the fuzz crate builds with `hevc`, so `video_frames` reaches it; patents stay M15.R's Q1 |
| `rusty_aac` =1.1.1, no defaults | Apache-2.0 | M15.15 (a clip's sound) | hostile audio packets | 36 in 4 files (`set_len` after full writes; the rest behind `simd`, off) | packets bounded by §4.2; accepted behind the fuzz gate (M15.R Q2): `video_sound` reaches it at every review |
| `nanomp3` =0.2.0, `lewton` =0.10.2 (`ogg` 0.8.0), `claxon` =0.4.3, `hound` =3.5.1 | MIT OR Apache-2.0; MIT OR Apache-2.0 (BSD-3-Clause); Apache-2.0; Apache-2.0 | M15.27 (slideshow music) | hostile music files | `nanomp3` and `lewton` `forbid(unsafe_code)`; `claxon` and `hound` a few sites each | `lewton` and `claxon` unreleased since 2021 and 2020 (record §8 Q5). `music` fuzzes all four at every review; it found `lewton` reserving what a Vorbis header declares before reading it (an abort), so Dalica reads the headers first through `ogg` (now a direct dependency, same pin) and refuses sizes the packets cannot hold |
| `pulseaudio` =0.3.2 | MIT | M15.27 (sound on Linux) | the local sound server's replies | 2 | message types only; the protocol client is our own |
| `cpal` =0.18.2, no defaults (with `coreaudio-rs`, `objc2-core-audio*`, `objc2-audio-toolbox`, `objc2-avf-audio`, `mach2`, `dasp_sample`) | Apache-2.0 | M15.27 (sound on Windows and macOS) | the system's audio APIs | about 200 | only its WASAPI and CoreAudio hosts; never on Linux, where it would link ALSA's C library (`alsa`, `alsa-sys` are in `Cargo.lock` only) |
| `nusb` 0.2.8 | Apache-2.0 OR MIT | M15.9 (cameras over USB) | a USB device | about 350 | Linux and Windows only (macOS would need `io-kit-sys`, not in §8.1's allow-list); behind the engine's default `camera-usb` feature (M15.52) so fuzz builds leave it out |
| `drag` =2.1.1 (with `windows` 0.52, `dunce`, `core-graphics` 0.24) | Apache-2.0 OR MIT | M15.23 (dragging photos out on Windows and macOS) | the OS's drag session | 37 | pinned; its GTK path for Linux is in `Cargo.lock` but never resolved (`cargo tree -i gtk-sys` prints nothing on any target) |
| `x11rb` 0.13.2 (already in the graph through winit and arboard) | MIT OR Apache-2.0 | M15.23 (the X11 drag source) | the X server | about 76, compiled in: winit turns on its `allow-unsafe-code` feature in the product graph | our own XDND source over its pure-Rust connection; Dalica's code calls only its safe API |
| `fmv-macos-events` =0.1.0 | MIT OR Apache-2.0 | M15.18 (Finder's Open With, the Dock) | Apple events | 12 | objc2 only; pinned as PhotoCraft pins it |
| `wl-clipboard-rs` 0.9 (with `os_pipe`, `tree_magic_mini`, `nom` 8, `petgraph`, `fixedbitset`, `hashbrown` 0.15, `foldhash`); features of `arboard`, `zbus` and `objc2-app-kit` | MIT or Apache-2.0 | M15.12 (the clipboard, the file manager's D-Bus call, macOS handler lists) | the desktop session | as reviewed in M15.12 | files on the Wayland clipboard; macOS handlers through `objc2-app-kit`'s safe `NSWorkspace` methods |

Advisories against all of these are checked by `cargo xtask advisories` (§8.4); the only recorded
exceptions are the two of §8.4 (`rsa`, `rustls-webpki` 0.102), neither brought by M15's crates.

## 9. Security testing

- `cargo xtask ci` on every task (never-crash lints, `#![forbid(unsafe_code)]`, the changed
  packages' tests, `xtask deps`); `cargo xtask ci --full` and fuzzing at every milestone review.
- Every command's hostile params: one engine test over every registered command, and `panic_hunt`
  (full case count at the review).
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

Releases follow the Craft apps' workflow (`process.md` §7). **Implemented (M6.13:
`.github/workflows/release.yml`, `packaging/checksums.sh`; the packages M6.21–M6.23; Windows
signing M15.10, macOS signing and notarization M15.13).** [`releasing.md`](releasing.md) describes
every job.

- A `release` branch builds platform packages (AppImage, Flatpak, deb/rpm, tarball, MSI, portable
  zip, DMG) and creates or updates a **draft** release; a maintainer publishes it after checking.
  Building, signing, drafting, publishing and deploying are separate states. Every other run (a
  dispatch on another branch, a pull request touching `packaging/`, the workflows or the icons) is
  a dry run that builds, checksums and verifies but signs and drafts nothing. A version whose
  release is already published is refused. **Implemented (M6.13).**
- Workflow permissions default to `contents: read`, and every action is pinned to a full commit
  hash. Build jobs that read no secrets run without the `release` environment, so a branch dry run
  can run them. Only the signing jobs use the `release` environment with its secrets, and only the
  final draft job gets `contents: write` (it runs without the environment and is reachable only
  through the signing jobs on the `release` branch). Workflow inputs reach scripts only through
  environment variables. **Implemented (M6.13).**
- Signing and notarization (macOS, Windows) are conditional: every signing secret is optional, and
  a missing one produces an unsigned artifact and a warning, never a failed build. The job summary
  states the signature state of every artifact; the release notes say it by hand
  (`releasing.md` step 5). Consumers verify the signature rather than assume one. **The jobs and
  their warnings are implemented (M6.13); the signing itself is implemented (M15.10, M15.13):**
  - Windows (`sign Windows x64`): the build job's tested programs are signed, the MSI is built
    around them and signed, and `test-sign.ps1` checks every signature; a release run signs with the
    `.pfx` (`WINDOWS_CERTIFICATE`) or Azure Trusted Signing (`AZURE_*`), timestamped. Measured by
    the dry run of #1046 (release run 39331055880: both programs, the MSI and the programs inside it
    signed with a throwaway certificate and `Valid`).
  - macOS (`sign macOS`): `Dalica.app` signed inside-out with the hardened runtime, a secure
    timestamp and empty entitlements (no `--deep`), the DMG and the CLI zip signed; with a
    Developer ID and the three Apple secrets the app, the DMG and the zip are notarized
    (`notarize.sh`) and the app and the DMG stapled; `verify.sh` checks them as users download them
    (after a `ditto` copy); the temporary keychain is deleted whatever happens. Measured by the dry
    run of #1068 (release run 39356962443).
  - Signing runs in its own job from the build job's stage, so the secrets never reach a job that
    runs cargo. A dry run never reads them: each is `real && secrets.X || ''`, and it signs with a
    throwaway self-signed identity made in the job and trusted on that runner only (notarization,
    stapling and Gatekeeper's check are skipped and named).
  - **Not yet done:** no release run has used the real Windows certificate or the Apple Developer
    ID, so the real signatures, notarization and stapling are unmeasured; they are checked at
    M15.R with the maintainer's secrets. Until a release run signs, artifacts are unsigned (ad hoc
    on macOS) and the warnings say so.
- Every release carries `SHA256SUMS.txt` over all artifacts, written once the set is complete,
  re-downloaded and verified on Linux, Windows and macOS (with the script's planted-mismatch
  self-test) and verified again by the draft job. **Implemented (M6.13).**
- Builds use the pinned toolchain and `--locked`; each package's binaries must report the version
  and the commit (`DALICA_BUILD_SHA`) or the build fails. **Implemented (M6.13).**
- Dalica has no built-in updater; updates come through the release page or the package channel
  (the AppImage carries update information for the separate AppImageUpdate tool).
- Packages request the narrowest platform permissions that still let users add their own folders.
  **Decided (M6.21–M6.23):** the Flatpak gets Wayland with X11 fallback, IPC, the GPU and
  read/write access to `xdg-pictures`, and no network and no home folder (a folder elsewhere needs
  a user's `flatpak override`, since the Folder Manager does not use the portal file chooser yet);
  the MSI registers Dalica under "Open with" without taking over a default; the macOS app has empty
  entitlements and the hardened runtime, and registers its formats as Alternate handler only.
  Since M15.24 the Linux desktop entry and the macOS `Info.plist` declare Dalica a viewer of the
  types it shows, and the MSI declares Capabilities and RegisteredApplications so Windows' Default
  apps can offer it; none of them takes a default by itself (§5.11).
  Packaging tools downloaded in CI (nfpm, appimagetool, its runtime, actionlint) are pinned by
  SHA-256; WiX is pinned by version (5.0.2).

- Every release carries a CycloneDX SBOM per package, and its crates are checked against the
  RustSec advisories before the checksums are written (§8.5). **Implemented (M15.40).**

Future hardening: provenance attestations tied to the source commit and workflow, explicit
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
