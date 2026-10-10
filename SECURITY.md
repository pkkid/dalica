Adapted from PhotoCraft (`SECURITY.md`, `book/src/security/*`, revision 59684266; MIT OR Apache-2.0).

# Security Policy

Dalica is a desktop photo organizer. It reads photos, videos, embedded metadata (EXIF, XMP, IPTC,
ICC) and sidecar files that may come from anywhere: cameras, phones, messengers, the web, other
people's computers, network shares, synced cloud folders and the legacy organizer it replaces. It
writes into the user's own photo folders, and it exposes a local JSON-lines control channel, a CLI
and an MCP server so AI agents can drive it. Treat files, file names, paths, metadata, sidecars,
automation requests and command parameters as potentially malicious.

The engineering side of this policy (threat model, trust boundaries, parser limits, file-handling
rules, automation limits, fuzzing, dependencies, releases) is in
[`docs/security.md`](docs/security.md).

## Supported versions

Dalica has no releases yet; the repository holds specifications and, from milestone M0 on, code.

| Version | Security fixes |
|---|---|
| `main` | yes |
| latest 0.x release (once one exists) | yes |
| older 0.x releases | no: upgrade to the latest release |

Before 1.0 only the newest release line receives fixes. This table will be revisited at 1.0.

## Reporting a vulnerability

Please give the maintainers a reasonable opportunity to investigate and coordinate a fix before
publishing exploitable details, proof-of-concept code or weaponized files.

The project does not yet have a dedicated private security address or private reporting channel.
TODO(maintainer): enable the hosting platform's private vulnerability reporting (or publish a
security contact) when the repository becomes public, and update this section.

Until then, contact the maintainers through the owner/maintainer channels listed for the
repository, give only a minimal non-exploitable summary in public, and ask for a private way to
send the details. Do not open a public issue that contains a working exploit, a token or other
secret, a sensitive crash dump, a malicious attachment, or someone's personal photos.

If no private channel can be established, withhold weaponized material and report the smallest
safe description that lets the maintainers get in touch.

## What to include

A useful report includes:

- the affected Dalica version, commit or release artifact;
- operating system, architecture, build profile and relevant feature flags;
- the affected component (crate, command id, control-channel method or MCP tool) or file path in
  the repository;
- the security impact and the access an attacker needs (a file the user opens or scans, write
  access to a shared folder, a local process, an MCP client, …);
- exact reproduction steps;
- expected and actual behaviour;
- crash logs, backtraces, sanitizer output or resource measurements when applicable;
- whether the problem reproduces from a clean checkout;
- any known workaround or containment.

Remove credentials, control tokens, personal paths, people's names, captions, location data and
unrelated system information from logs before sending them.

## Malicious test files

Do not attach a potentially harmful or non-redistributable file to a public issue. First provide
its format, size, SHA-256 hash, observed effect, provenance category (for example synthetic,
fuzz-generated or camera-produced) and the command needed to reproduce the problem. Coordinate a
private transfer method with the maintainers before sending the sample.

Never send a real personal photo. Photos carry faces, locations (EXIF GPS), device serial numbers
and captions. Reduce a sample to a synthetic file, or strip it to the smallest set of bytes that
still triggers the problem.

Test files accepted into the repository must be minimized, synthetic or CC0, free of personal or
proprietary data, legally redistributable, and documented with the expected failure behaviour. A
fixed parser or file-handling defect receives a permanent regression test whenever that is safe
and practical.

## Scope

Security reports may cover:

- image decoding and sniffing (JPEG, PNG, TIFF, WebP, GIF, BMP, HEIC/HEIF, AVIF, JPEG XL, PSD
  composites, embedded RAW previews), embedded thumbnails and ICC profiles;
- EXIF/TIFF, XMP and IPTC parsing, and metadata write-back (merge, corruption, loss of foreign
  fields);
- the folder sidecar (`.dalica.json`), the originals backups (`.dalica-originals/`), legacy sidecar
  reading, and the library index, journal and settings files;
- decompression bombs, oversized dimensions, integer overflow, memory exhaustion, deep recursion,
  hangs and directory-walk exhaustion;
- file-system handling: path traversal in sidecar or file names, symlinks and junctions, unsafe
  overwrite or delete behaviour, temporary files, network shares and removable media;
- anything that can destroy, corrupt or silently replace a user's original photo;
- engine command validation and serialization/deserialization;
- the desktop control channel, the headless CLI, the MCP server and its connection to the running
  app: authentication, token handling, request/connection/batch limits, file-system authority;
- generated output that other programs open (exported files and their names, collage recipes);
- network features once they exist (map tiles, geocoding, model downloads);
- dependencies, CI, packaging, signing, checksums and release-artifact integrity.

Not in scope:

- attacks that already require running code as the same OS user with access to the photo folders
  (such a process can read and change the photos directly);
- "hidden" photos and folders being visible to someone with file-system access: hiding is a view
  filter, not access control;
- data in a folder sidecar being readable by everyone who can read that folder: sidecars travel
  with the folder by design;
- bugs in the operating system, its trash, the system mail client, file manager or other external
  programs Dalica launches at the user's request;
- general bugs without a security impact: use the normal issue process.

## Current status

Nothing is built yet. The controls described in [`docs/security.md`](docs/security.md) are
planned per milestone (marked **Planned (M0)**, **Planned (M1)**, …) and become **Implemented**
only when their code and tests land on `main`. Do not rely on a planned control. In particular,
the control channel is designed to require a 256-bit token, bind only to loopback, reject
anything that is not a JSON request line, and enforce request, connection and batch limits from
its first commit (M2); the MCP server is designed to confine file-system access to granted roots
(M10).
