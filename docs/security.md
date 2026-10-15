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
> release workflow (signing excepted). The commit that lands a control
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
| Folder sidecar `.dalica.json` | size/depth/entry limits, single-component names only, escaped values, unknown data preserved; fuzz + property tests | M1 (property tests; the fuzz targets are not written yet, §7.1) | a sidecar is as private as its folder |
| Legacy sidecars and backups | read-only fallback when a folder has no `.dalica.json`, same limits, no path from them is ever opened | M14 | |
| Library index, journal, settings | checksummed records, size and depth limits, `.corrupt-<time>` quarantine | M1 | |
| Directory walks | depth 32, 1,000,000 entries, symlink-loop detection, off the UI thread | M1 | |
| File writes | atomic replace with identity check, no writes through symlinks, OS trash only | **Implemented** for sidecars (M1) and metadata write-back (M6.30: re-read, merge, verify, `fs::replace` expecting the identity read, the modification time put back; a root checked right before each write, M6.43); M8 (Save), M9 (file operations) | network file systems weaken atomicity (§5.9); a process killed between the rename and putting the time back leaves the new file with the write's time |
| Control channel | loopback only, 256-bit token from a `0600` file, JSON-request-only lines, request/reply/connection/batch limits | M2 | no per-client capabilities, no encryption |
| MCP server | stdio by default, roots, read-only mode, tool annotations, same limits | M10 | an authenticated agent can still trash or caption any photo in the library unless read-only |
| Network features | user-initiated only, TLS, size limits, pinned model hashes | M13 | TLS stack still to be decided (§8.2) |
| Generated output | escaped JSON, sanitized file names | M8 (export), M14 (collage) | |
| Dependencies | pure Rust, banned crates, licence allow-list, advisory check | M0 | scanner results do not replace review |
| Releases | draft releases, `SHA256SUMS.txt`, optional signing secrets, least-privilege jobs | **Implemented** (M6.13 `.github/workflows/release.yml`, `packaging/checksums.sh`; packages M6.21–M6.23); signing and notarization M15 | until M15 every artifact is unsigned (ad hoc on macOS); consumers verify `SHA256SUMS.txt` |

The table is a plan, not a guarantee about any build. Verify source and release evidence before
relying on it.

### 1.2 Priorities

1. M0–M2: the control channel ships with authentication, line validation and limits from its first
   commit; there is no unauthenticated phase. The never-crash lints, `panic_hunt` (M1) and the
   dependency check (§8) land with the workspace.
2. M1: bounded decoders and walkers, the sidecar parser (its fuzz target still to come, §7.1), and one shared
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
  video containers (M15) / face models (M13) / map tiles and geocoder replies (M13)

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
| Legacy sidecars | `sidecar` | file ≤ 64 MiB; line ≤ 1 MiB; ≤ 1,000,000 sections; ≤ 1,000 keys per section; the recipe limits above; encoding detection never fails the whole file | M14 |
| JSON files (`folders`, `albums`, `people`, `settings`) | `library` | file ≤ 64 MiB; depth ≤ 64 | M1 |
| Index journal / snapshot | `library` | checksummed records ≤ 16 MiB; a torn tail is discarded, a bad checksum stops replay at that record; version checked | M1 |
| Control / MCP JSON | `ui`, `mcp` | see §6.5 | M2 / M10 |
| Video containers | `image` or the video crate | box depth ≤ 12, ≤ 4,096 boxes per level, sample tables bounded; or an external process with a timeout (reserved decision 2) | M15 |
| Face models | faces crate | SHA-256 pinned in the source; size cap; parsed as data only | M13 |
| Map tiles, geocoder replies | network module | tile ≤ 1 MiB and ≤ 1,024 px edge, decoded under the raster limits; JSON reply ≤ 1 MiB, depth ≤ 32 | M13 |

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
import folders (M11) and collage files (M14) will.)**

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
  as itself, never its target).** The commands that call them are planned (M9).
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
  reason).** The delete commands are planned (M9.15).
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
  explanation is the delete command's (M9.15).
- "Remove from Dalica", "Rebuild library" and cache pruning never touch photos. Caches list and
  delete only files of their own naming shape. Stale sidecar entries are pruned, not files.
- **Camera and card import** ("Delete copied files from the card"): cards usually have no trash,
  so this is the only permanent deletion Dalica performs. They
  happen only after every copied file has been verified by hash at its destination, only for files
  the import listed, after an explicit confirmation, and they are refused over automation.
  **Planned (M11).**
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
  the rename).** The move commands are planned (M9.13, M9.15).

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
  validated, and sizes reported by the device are not trusted for allocation. **Planned (M11).**

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
  on Disk). These commands are refused over automation (§6.6). **Planned (M15).**
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
| MCP over stdio, headless engine | `dalica-cli mcp [--demo]` | inherited from the process pipe | M10 |
| MCP bridge to the running app | `dalica-cli mcp --connect 127.0.0.1:<port>` | token (§6.3) | M10 |

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
  agents. **Planned (M10).**

Command policy and roots: **Planned (M2)** for `ui.screenshot`; **Planned (M8–M11)** for the
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

All of §6.7: **Planned (M10).**

### 6.8 Control status

| Control | Status |
|---|---|
| Loopback-only TCP binding | Implemented (M2; `control_security.rs`) |
| Token authentication from a `0600` token file | Implemented (M2; `control_security.rs`, `control_headless.rs`); the "owned by the current user" check of §6.3 is not yet built, and on Windows the file keeps its folder's ACL |
| JSON-request-only lines, close on junk | Implemented (M2; `control_security.rs`) |
| Request, reply, depth, connection, timeout limits | Implemented (M2; `control_security.rs`, the `ui` control tests; depth 64 since #102) |
| Screenshot / snapshot dimension limits | Implemented (M1, M2; the `ui` control tests) |
| Command error contract and `panic_hunt` | Implemented (M1; `panic_hunt`) |
| Root-checked path parameters | Implemented for screenshots (M2; the `ui` control tests); M8–M11 others planned |
| Batch limits, paging, preview limits | Planned (M10) |
| MCP roots, read-only mode, tool annotations | Planned (M10) |
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
| `crates/image` | `sniff`, `decode_any`, `decode_jpeg`, `decode_png`, `decode_tiff`, `decode_webp`, `decode_gif`, `decode_bmp`, `decode_jxl`, `decode_psd`, `decode_heif` (each format decoded explicitly, no sniffing), `raw_preview` (every RAW container's recognition and preview decode), `embedded_thumbnail` (EXIF thumbnails, thumbnail IFDs, PSD resources, RAW previews), `icc` | **Implemented** (M5.1, in `crates/image/fuzz/`; the targets reach each decoder as M5.6–M5.10 filled it; `decode_heif` reaches `heic-rs` only when built with `--features dalica-image/heif`, and the Fuzz workflow builds without it, so there it fuzzes our box walk); built and run for 60 s each at every milestone review and when the Fuzz workflow is dispatched |
| `crates/meta` | `exif` (IFDs and maker notes), `xmp` (including extended-XMP reassembly), `iptc`, `write_merge` (parse → merge-write → re-parse: no panic, foreign fields unchanged), `carry` (any file as the source of a rendered copy's metadata: an error, or a JPEG or PNG whose metadata reads back, orientation 1, the size asked, no GPS when the location was removed; setting the orientation reads back as asked) | **Implemented** (M6.4, in `crates/meta/fuzz/`; `carry` M8.7; `write_merge` reaches every container writer (JPEG M6.17, PNG, WebP and TIFF M6.18) and asserts that a returned write passes its verification, reads back and writes again to the same bytes; `xmp` reaches the XML reader (M6.9), the merge writer and extended-XMP reassembly with MD5 GUIDs, and writes its JPEG through `write_owned` (M6.17)); built and run for 60 s each at every milestone review and when the Fuzz workflow is dispatched. Local runs before merging (stable with cargo-fuzz's flags): `write_merge` 2.8 M runs in 180 s and `xmp` 0.8 M in 60 s with no finding after the one idempotence finding M6.17 fixed (`tests/write_jpeg.rs` keeps its regression test). The metadata corpus (M6.34, `crates/meta/tests/corpus.rs`) runs the same write rules over files other applications wrote |
| `crates/sidecar` | `parse`, `roundtrip` (parse → write → parse is stable, unknown data preserved), `recipe` (edit-operation list), `legacy` | **Not written** (planned for M1, never landed; no task yet; `legacy`: M14). Until then `crates/sidecar/src/tests.rs` runs property tests: random bytes never panic, print → parse is the identity, any file name round-trips as a key |
| `crates/store` | `journal` (replay of journal records and snapshots), `json` (folders, albums, people, settings) | **Not written** (planned for M1, never landed; no task yet). Until then the journal's tests read every prefix of it in memory and reopen the store at each record boundary (at every byte at full strength; `crates/store/src/lib.rs`), and tear the sketch file's tail (`crates/store/tests/sketch.rs`) |
| `crates/ui` control | `request_line` (line validation, auth state machine, limits) | **Not written** (planned for M2, never landed; no task yet) |
| `crates/mcp` | `tool_args` | M10 |
| video demuxer | `demux` (if a pure-Rust path is chosen) | M15 |

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

CI: the Fuzz workflow (`.github/workflows/fuzz.yml`, M5.1, M6.4) builds every `image` and `meta`
target and runs each for 60 s on the pull request of every milestone review (maintainer decision,
2026-10-15; before that the targets were built but never run), and for a chosen time when
dispatched by hand. A crash fails the review's run and keeps its input as an artifact; the fix is a
task merged before the review closes. A scheduled weekly run of every target for a fixed time lands
by M15. Continuous hosted fuzzing is future work.

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

Map tiles, geocoding and model downloads (M13) need TLS. The usual Rust TLS providers (`ring`,
`aws-lc-rs`) contain C or assembly, and LightCraft's lockfile already contains `ring`. Before M13 a
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
supported targets, maintenance status and licence. Reused Craft-app code (`architecture.md` §9)
and any other vendored code keep a header naming their origin and get a `NOTICE` row. A scanner
result does not replace reading the code. Community batches follow the checklist in `process.md`
§4 (new dependencies, `build.rs` or CI changes, `unsafe`, hidden Unicode, network access, file
writes outside the library).

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
`.github/workflows/release.yml`, `packaging/checksums.sh`; the packages M6.21–M6.23); signing and
notarization M15.** [`releasing.md`](releasing.md) describes every job.

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
  their warnings are implemented (M6.13); the signing itself is M15**, so until then every artifact
  is unsigned (ad hoc on macOS) and the warnings say so.
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
  Packaging tools downloaded in CI (nfpm, appimagetool, its runtime, actionlint) are pinned by
  SHA-256; WiX is pinned by version (5.0.2).

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
