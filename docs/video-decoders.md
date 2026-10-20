# Video and sound decoders: the M15.46 record

Reserved decision 2 (`roadmap.md`) was answered at M14.R (2026-10-19): Dalica decodes H.264 and
HEVC with pure-Rust decoders, each kept only if it passes a spike on real phone files; a codec that
fails stays a tile (probe, cell, the Videos filter, "This video can't be played" with Open With).
This file is that spike's record (M15.46, measured 2026-10-19). M15.15, M15.20, M15.25–M15.28,
M15.30 and M15.34 read it for which crate decodes what. Nothing here is product code, and no
dependency was added: the tasks that build on it add the crates named in §2, each through
`cargo xtask deps` and `security.md` §8.6.

> Status (M15.15, 2026-10-19): built. `dalica-image` decodes H.264 through `altc264` =0.1.1 in
> every build, HEVC through `rusty_h265` =0.6.2 (`simd`) behind its non-default `hevc` feature
> until §8's first question is answered, and AAC through `rusty_aac` =1.1.1 (no default features):
> `formats::video::frames::FrameSource` and `formats::video::sound::SoundSource` (`formats.md` §1,
> the Video row). `rusty_h264` is not used. The phone clips of §9 are pinned for the corpus job as
> the `video` set (`xtask/src/corpus_pins.rs`), not published. Measured through Dalica's own path
> (demuxing, the gate, conversion to upright 8-bit sRGB), release build, one core, on a machine at
> load 20–27 of 24 threads (floors, as §3 says): H.264 1080p (IMG_6851) 34–40 fps at full size,
> 66–68 fps at a 256 px long side, peak 92 MB; HEVC 1080p (IMG_0445) 24 fps full size, 34 fps at
> 256 px, 92 MB; HEVC Main 10 HLG 1080p60 (M-6) 8–12 fps, 101 MB (decoder-bound under that load;
> its tone mapping costs about 45 ms a 1080p frame on one core); H.264 4K60 with 8 threads
> 59 fps at a 1,280 px long side, 11.6 fps at full size, 0.74–1.2 GB (6.9 fps and 374 MB on one
> core); AAC: a 7.9 s stereo track in 24 ms.

## 1. The bar

From the task block, a codec **passes** when one crate

1. decodes every real phone file of its kind with frames identical to a reference decoder (H.264
   and HEVC decoding is exact by their standards; for sound, within the tolerance the standard
   gives),
2. decodes 1080p at least in real time on one core of the maintainer's machine (an i7-13700K),
3. passes `cargo xtask deps` without an exception (licence, no C build, no unlisted `*-sys`),
4. never panics or hangs on damaged input.

What a crate lacks is named. A candidate that passes only with a licence or dependency exception
counts as failing until the maintainer answers (§8). `unsafe` is counted, not judged: the bar does
not exclude it, so where a pass rests on `unsafe` code it is said, and §8 asks.

One limit came from running the spike: every decoder run was capped at 3 GB of address space
(`ulimit -v 3000000`), one run at a time, after an uncapped parallel run took the shared machine
down. A decoder that cannot decode a phone clip inside that cap fails on memory.

## 2. Verdicts

| Codec | Verdict | Crate (pin exactly) | Why |
|---|---|---|---|
| **H.264** | **passes** | `altc264` =0.1.1 | every phone file (6, up to 4K60) and every conformance stream measured exact, interlaced included; 61–105 fps at 1080p on one core, 173 fps at 4K60 on 8 threads; MIT, no dependencies; no panic or hang on damaged input. It rests on 257 `unsafe` sites in its decoding core (§7.1), and is five days old: §8 asks |
| **HEVC** | **passes** | `rusty_h265` =0.6.2 with its default `simd` feature | 147 of 147 conformance streams and every phone file exact (4 iPhone 7 Plus clips, 2 iPhone 13 HDR clips with Dolby Vision, 2 Main 10 clips made from them); 51–56 fps at 1080p30 on one core with `simd`, 22–31 fps without it (below real time); the iPhone 13 HDR clips are 1080p**60** and decode at 48 and 59 fps, just under real time, and 4K60 at 22 fps: the crate has no threading (§6, §8); Apache-2.0, no dependencies but its own `rusty_h265-accel`; no panic or hang. Without `simd` it is `#![forbid(unsafe_code)]` throughout; `simd` adds 702 `unsafe` sites of AVX2/NEON kernels chosen at run time. Patents: §8 |
| **AAC-LC** (phone sound) | **passes** | `rusty_aac` =1.1.1, `default-features = false` | every phone track and ISO conformance stream within full accuracy of ffmpeg's float decode; 370–3,200× real time; Apache-2.0, no dependencies; no panic or hang |
| **MP3** | **passes** | `nanomp3` =0.2.0 | full accuracy on every file, gapless trimming as ffmpeg's; 1,370–3,730× real time; MIT OR Apache-2.0; its own crates `#![forbid(unsafe_code)]` (its SIMD comes from `wide`/`safe_arch`) |
| **Ogg Vorbis** | **passes** | `lewton` =0.10.2 | within 3.7e-7 of ffmpeg on every file (the Vorbis specification gives no tolerance); 465–790× real time; MIT OR Apache-2.0 (`ogg` BSD-3-Clause); `#![forbid(unsafe_code)]` as Dalica would build it. Last release January 2021 |
| **FLAC** | **passes** | `claxon` =0.4.3 | bit-exact on every file (16 and 24 bits); 880–1,220× real time; Apache-2.0. Last release August 2020; `rusty_flac` 0.2.1 is bit-exact too and the alternative |
| **WAV** | **passes** | `hound` =3.5.1 | bit-exact (16, 24-bit and float); Apache-2.0 |

No codec falls back to tiles only on these measurements. Every later task still keeps the tile
path for what these crates refuse (§7: 4:2:2 and 4:4:4 video, H.264 4:0:0 not measured, HEVC range
extensions, other codecs), and the maintainer's answers in §8 can still turn a pass into tiles.

## 3. How it was measured

A throwaway harness (`vdspike` for video, `aharness` for sound; outside the tree, their sources are
attached to the pull request) feeds each crate the coded stream and compares what comes out with
ffmpeg 8.1.3's decode of the same stream (run by hand as a reference only, never linked).

- **Video correctness:** each crate's frames in display order (its own order where it gives one,
  else by coded video sequence and picture order count), cropped to the visible picture, compared
  frame by frame (identical, or the PSNR of each differing frame) and as one MD5 of the whole
  output against ffmpeg and, for the conformance suites, the published MD5. The streams were cut
  out of the phone files with ffmpeg's `*_mp4toannexb` filters: the spike measures codecs, not a
  demuxer (Dalica's own bounded ISO BMFF walk does that).
- **Sound correctness:** the decoded samples against ffmpeg's float (`f32le`) decode, aligned on
  the best offset within ±4,096 samples (reported when not 0): the RMS and largest difference at
  full scale ±1, and the MPEG "full accuracy" criterion (ISO/IEC 11172-4 and 14496-26, 16-bit
  output: RMS of the difference ≤ 2⁻¹⁵/√12, largest ≤ 2⁻¹⁴). FLAC and WAV must be bit-exact.
- **Speed:** frames per second decoding only (no conversion, no output), release build, pinned to
  one core (`taskset`), and with the crate's own threading on 8 cores where it has one. Other
  agents shared the machine during the runs (load 10–14 on its 24 threads), so the numbers are a
  floor.
- **Memory:** the peak resident set of the run (`/usr/bin/time`), which includes the whole coded
  stream held in memory.
- **Robustness:** two phone streams per video codec: cut at half and at a third plus 7 bytes,
  three copies with 50 random bits flipped past the first kilobyte (fixed seeds), and real
  headers followed by 200 KB of random bytes; for the two crates that pass, 30 more copies each
  with 5 to 500 random bytes overwritten. Sound: every file cut at half and at an odd byte, and
  three copies with ~50 bits flipped. Each run under a timeout (120 s video, 20 s sound) and the
  3 GB cap; a panic is caught and reported, an abort or a timeout counts against the crate.
- **`unsafe`:** `unsafe` keywords in the crate's source (comments, strings, tests and examples
  excluded), and in what it pulls in to decode.
- **`cargo xtask deps`:** on a scratch worktree with the crate added to `apps/dalica-cli`, one
  crate at a time, as each feature set would be used.

## 4. Conformance suites

### 4.1 HEVC: ITU-T H.265.1 (JCT-VC HEVC_v1, 147 streams, Main and Main 10)

Against the published YUV MD5 of each stream (and ffmpeg's output):

| Crate | Exact | Differs | Panics | Notes |
|---|---|---|---|---|
| `rusty_h265` 0.6.2 | **147** | 0 | 0 | the same with and without `simd` |
| `zvidlib-hevc-decoder` 0.5.0 | 130 | 17 | 0 | wrong pixels in DBLK_A–C, DELTAQP_A/B, LTRPSPS_A, RPLM_A/B, RPS_D/E, SLICES_A; BUMPING_A, NoOutPrior_A/B, RAP_B and CONFWIN_A partly reflect the harness driving its internal engine API (picture output and cropping) |
| `rust_h265` 0.1.0 | 41 | 99 | 7 | fails weighted prediction, tiles, wavefronts, SAO, merge and most reference-list streams |

ffmpeg itself differs from the published MD5 on SAODBLK_A and SAODBLK_B (`rusty_h265` and `zvidlib`
match the MD5), cannot decode TSUNEQBD_A (10-bit luma, 9-bit chroma; both match the MD5), and
VPSSPSPPS_A changes the picture size every frame (`rusty_h265`'s own CLI output matches the MD5).

### 4.2 H.264: ITU-T H.264.1 (JVT AVCv1 and FRExt, 177 streams)

All of AVCv1 but the 264 MB MV1_BRCM_D, and every 8-bit 4:2:0 FRExt stream (High profile); the
4:2:2, 4:4:4, 10-bit and alpha sets were left out (phones do not record them). Against ffmpeg and
the suites' decoded YUV:

| Crate | Progressive, exact of 118 | Interlaced, exact of 59 | Notes |
|---|---|---|---|
| `altc264` 0.1.1 | **116** | **59** | the other 2 are 4:0:0 (monochrome) streams the harness skipped |
| `rusty_h264` 0.16.1 | 71 | 2 | wrong pixels on High-profile CABAC streams (HCHP1/2, HCMP1, CANL2/3, CAPM3, NL2…); refuses MBAFF, field pictures, FMO/ASO, SP/SI |
| `z264` 0.1.0 | 48 | 2 | |
| `rust_h264` 0.4.0 | 46 | 2 | |
| `ps-rust_h264` 0.4.0 | 44 | 2 | |
| `hibernia` 0.1.0 | 12 | 0 | panics (`todo!`) on any profile above Baseline |

## 5. Phone files

Frames identical to ffmpeg's, in display order, of the frames in the clip ("exact" = all of them):

| File | Phone, codec | `altc264` | `rusty_h264` | `rust_h264` | `ps-rust_h264` | `z264` | `hibernia` |
|---|---|---|---|---|---|---|---|
| `IMG_6851.MOV` | iPhone 13, H.264 High 1080p | exact (221) | exact | exact | exact | exact | panic |
| `20260418_0635.MOV` | iPhone 7 Plus, H.264 High 1080p | exact (2,492) | exact | not run | not run | not run | panic |
| iPhone 12 mini clip | iPhone 12 mini, H.264 High **4K60** | exact (1,174) | exact | exact | not run | not run | panic |
| `20260404_201346.mp4` | Android 14, H.264 High 1080p | exact (872) | exact | every frame's pixels exact, display order wrong (31 of 872 in place) | the same | the same | panic |
| `VID_20261017_160853309.mp4` | moto g play, H.264 High 1080p | exact (484) | exact | exact | exact | exact | panic |
| `VID_20261017_141936777.mp4` | moto g play, H.264 High 1080p | exact (172) | exact | exact | exact | exact | panic |

The `rust_h264` family does not carry the picture order count's most significant part, so when
the count wraps (every 16 frames on this Android clip, whose `max_pic_order_cnt_lsb` is 32) its
reordering mixes the frames up (in its `OrderedDecoder` as in the harness); every pixel is still
right, and a player ordering by the container's composition times would not see it.
They were left out of the two longest clips (8–28 fps; five phone files already measured).

| File | Phone, codec | `rusty_h265` | `zvidlib-hevc-decoder` | `rust_h265` |
|---|---|---|---|---|
| `IMG_0444.MOV` | iPhone 7 Plus, HEVC Main 1080p | exact (51) | exact | refused (0 frames) |
| `IMG_0445.MOV` | iPhone 7 Plus, HEVC Main 1080p | exact (706) | **out of memory** (3 GB cap) | refused |
| `IMG_0477.MOV` | iPhone 7 Plus, HEVC Main 1080p, portrait | exact (628) | out of memory | refused |
| `IMG_0488.MOV` | iPhone 7 Plus, HEVC Main 1080p, portrait | exact (1,100) | out of memory | refused |
| `M-6 Fun Fact.MOV` | iPhone 13, HEVC **Main 10, HLG, Dolby Vision** profile 8.4, 1080p60, portrait | exact (2,167) | out of memory | 1,049 of 2,167 in order, then wrong (13 dB) |
| `104-107 Fun Fact.MOV` | iPhone 13, HEVC Main 10, HLG, Dolby Vision, 1080p60 | exact (5,055) | out of memory | 35 frames, then refused |
| `gen_1080_main10_hlg.hevc` | made from `IMG_0445`, Main 10 HLG, B-frames | exact (300) | out of memory | 86 of 300 (worst 11 dB) |
| `gen_4k_main10_hlg.hevc` | made from the 4K clip, Main 10 HLG 2160 × 3840 | exact (240) | out of memory | 4 of 240 (59 errors) |

`rust_h265` refuses every iPhone 7 Plus clip ("slice_pic_parameter_set_id does not match active PPS"). The
Dolby Vision clips carry their metadata (the RPU) as NAL units of type 62, which `rusty_h265`
skips as the standard says; what is decoded is the HLG base layer every player shows.

## 6. Speed and memory

Decoding only, frames per second (the clip's own rate in brackets), one core unless the column
says 8 threads; peak resident memory of the one-core run.

| Crate (build) | iPhone 13 1080p (29.97) | moto g 1080p (29.6) | Android 14 1080p (30) | iPhone 12 mini 4K (59.94) | Peak memory, 1080p / 4K |
|---|---|---|---|---|---|
| `altc264` | **61.6** (112 rerun on a quieter machine) | **72.4** | **105.3** | 42.4 | 49–81 MB / 239 MB |
| `altc264`, 8 threads | 212.6 | 304.9 | 385.7 | **173.3** | 246–288 MB / 942 MB |
| `rusty_h264`, no defaults | 25.3 | 34.5 | 39.7 | 16.1 | 60–120 MB / 593 MB |
| `rusty_h264`, `asm` | 41.7 | 57.2 | 59.4 | 31.9 | as above |
| `rusty_h264`, its frame threads (8) | 25.9 | 29.8 | 34.2 | 24.3 | 407–893 MB / 1.7 GB (grows with the clip) |
| `z264` | 22.3 | 19.9 | 28.5 | 10.0 | 43–88 MB / 193 MB |
| `ps-rust_h264` | 17.1 | 20.9 | 27.0 | 8.0 | 47–92 MB / 193 MB |
| `rust_h264` | 17.4 | 17.7 | 21.9 | 7.9 | 47–91 MB / 196 MB |

| Crate (build) | `IMG_0445` 1080p Main (30) | `IMG_0488` 1080p Main (30) | iPhone 13 HDR 1080p Main 10 (60): M-6, 104-107 | made 1080p Main 10 HLG (30) | made 4K Main 10 HLG (60) | Peak memory, 1080p / 4K |
|---|---|---|---|---|---|---|
| `rusty_h265`, `simd` (default) | **54.6** (55.5 rerun) | **56.0** | 48.3, 59.0 | **50.6** | 21.8 | 65–225 MB / 156 MB |
| `rusty_h265`, no defaults | 24.3 | 31.0 | 23.4, 26.5 | 21.7 | 9.2 | as above |
| `zvidlib-hevc-decoder` | out of memory | out of memory | out of memory | out of memory | out of memory | > 3 GB |
| `rust_h265` | refused | refused | wrong | | | |

So at 1080p30 on one core `altc264` and `rusty_h265` with `simd` run at about twice real time,
`rusty_h264` reaches it only with `asm`, and `rusty_h265` without `simd` does not on three of four
clips. At 1080p60 (the iPhone 13 HDR clips) `rusty_h265` is just under real time (48 and 59 fps),
and at 4K60 neither codec is real time on one core: `altc264` is with its own threads (173 fps);
`rusty_h265` has none, so 1080p60 and 4K HEVC need frame or slice threading of Dalica's own (or
play below their rate). Every memory peak includes the whole coded stream (14–155 MB) held by the harness.

### 6.1 Damaged input

| Crate | Runs | Panics | Hangs | Aborts |
|---|---|---|---|---|
| `altc264` (one core) | 12 damaged + 30 fuzzed | 0 | 0 | 0 |
| `altc264` (8 threads) | the same 42 | 0 | 0 | 0 |
| `rusty_h265` | 12 damaged (no defaults) + 30 fuzzed in each build + 5 damaged HDR clips in each build | 0 | 0 | 0 |
| `rusty_h264` | 12 damaged (no defaults) + 30 fuzzed (`asm`) | 0 | 0 | 0 |
| `rust_h264`, `ps-rust_h264`, `z264`, `zvidlib-hevc-decoder`, `rust_h265` | 12 each | 0 | 0 | 0 |
| `hibernia` | 12 | 12 (`todo!`, Main/High) | 0 | 0 |

Every refusal came back as an error value. Sound: 15 damaged copies per AAC, Vorbis and FLAC
crate, 25 per MP3 crate and 20 per WAV crate: one panic (`puremp3`), no hang, no abort. This is a
smoke test, not fuzzing: the wrapper that calls a passing crate still gets fuzz targets of its
own, as `security.md`'s table asks of image decoding.

## 7. Every candidate

### 7.1 H.264

| Crate | Licence | `xtask deps` | `unsafe` | Releases, maintenance | Lacks, risks |
|---|---|---|---|---|---|
| `altc264` 0.1.1 | MIT | ok, no dependencies | 257 in 15 files: AVX2/NEON intrinsics, and in the core raw-pointer slices of shared pictures, `get_unchecked`, a lifetime `transmute` in its thread pool; `#![forbid(unsafe_op_in_unsafe_fn)]` only | 2 releases, first 2026-10-14; one maintainer; 1 star | 4:0:0 not measured; also decodes MVC stereo, 4:2:2/4:4:4 and high bit depths (not measured) |
| `rusty_h264-decoder` 0.16.1 | BSD-2-Clause | ok with and without defaults | 0 in the decoder, 1 in `-common`; its `asm` feature adds `rusty_h264-accel` (238) | 18 releases since 2026-06-27; 12 open issues | wrong pixels on High-profile CABAC conformance streams, no interlace; the **default features install a process-wide global allocator** (`rusty_alloc`, its `global-alloc` feature): never build it with defaults |
| `rust_h264` 0.4.0 | MIT OR Apache-2.0 | ok | 6, NEON only (none built on x86-64) | 4 releases, last 2026-04-20 | does not carry the picture order count's most significant part: on the Android clip its display order is wrong (pixels exact); 46 of 118 progressive conformance streams |
| `ps-rust_h264` 0.4.0 | MIT OR Apache-2.0 | ok | 6, NEON only | 1 release (a fork of `rust_h264`, 2026-10-13) | as `rust_h264` |
| `z264` 0.1.0 | MIT OR Apache-2.0 | ok | 10 (a `OnceCell`, NEON) | 1 release (adapted from `rust_h264`, `no_std`) | as `rust_h264` |
| `hibernia` 0.1.0 | MIT | ok | 0 (`v_frame` dependency: 8) | 1 release | Baseline only; `todo!()` panics on Main and High |

Not tried, by their own documentation: `sillstone-h264` 0.2.1 (Baseline only, no CABAC),
`oxideav-h264` 0.1.9 ("no decode functionality yet"), `media-codec-h264` 0.1.1 and `h264-decoder`
0.2.5 (parameter-set parsers, no picture decoding), `rff-codec-h264`, `rumpeg-h264` and
`videoson-codec-h264` (wrappers of crates above), `rff-codec-openh264` (C).

### 7.2 HEVC

| Crate | Licence | `xtask deps` | `unsafe` | Releases, maintenance | Lacks, risks |
|---|---|---|---|---|---|
| `rusty_h265` 0.6.2 | Apache-2.0 | ok with and without defaults | 0 (`#![forbid(unsafe_code)]`); `rusty_h265-accel` 0.6.2 is `forbid(unsafe_code)` without `simd` and has 702 with it (AVX2/NEON, chosen at run time by CPU detection) | 8 releases since 2026-09-06; 3 open issues | Main, Main 10 and Main Still Picture only: range extensions (4:2:2, 4:4:4, 12-bit), SCC and layered profiles are refused by name; single-threaded |
| `zvidlib-hevc-decoder` 0.5.0 | MIT | ok (its default `hardware` feature adds platform hardware-decoder bridges) | 0 in the crate; `zvidlib-hevc` 184, `zvidlib-core` 204, `zvidlib-color` 84 | 2 releases, first 2026-10-07 | 17 conformance failures; holds every picture as 32-bit samples and ran out of the 3 GB cap on every phone clip but the 51-frame one |
| `rust_h265` 0.1.0 | MIT OR Apache-2.0 | ok | 0 | 1 release, 2026-04-22; 2 open issues | refuses every iPhone clip ("slice_pic_parameter_set_id does not match active PPS"); 41 of 147 conformance streams; 7 panics |

Not tried: `oxideav-h265` 0.0.15 (a "decoder scaffold"), `media-codec-h265` 0.1.1 (a parser),
`rff-codec-hevc` and `videoson-codec-h265` (wrappers), the HEIF still-image decoders (`heic-rs`,
`heifer-hevc-dec`, `zune-heic`: intra pictures only), `libde265-rs` (C).

### 7.3 Sound

| Format | Crate | Licence | `xtask deps` | `unsafe` | Accuracy (worst file) | × real time (one core) | Damaged input (15–25 files) | Maintenance |
|---|---|---|---|---|---|---|---|---|
| AAC | `rusty_aac` 1.1.1, no defaults | Apache-2.0 | ok | 36 in 4 files (`set_len` after full writes; the rest behind `simd`) | full accuracy on all 9: largest 3.0e-7, RMS 2.7e-8 | 370–3,200 | no panic, no hang | 8 releases since 2026-07-29 |
| AAC | `symphonia-codec-aac` 0.6.1 | **MPL-2.0** | **fails** (licence) | 0 | full accuracy on the 3 phone tracks; the harness's raw ADTS path refused the 6 ISO streams | 1,600–2,650 | no panic | mature (2021–) |
| AAC | `oxideav-aac` 0.1.7 | MIT | ok | 0 (+2 in `oxideav-core`) | wrong on 4 of 6 ISO streams (RMS up to 0.18) | **0.2–1.9** | no panic | 17 releases |
| MP3 | `nanomp3` 0.2.0 | MIT OR Apache-2.0 | ok | 0 in its crates; `wide` 443, `safe_arch` 1,157 (SIMD wrappers) | full accuracy on all 5; largest 1.2e-6, RMS 4.7e-8 | 1,370–3,730 | no panic | 4 releases, last 2026-09-30 |
| MP3 | `rusty_mp3` 1.0.1 | Apache-2.0 | ok | 75 in 4 files | full accuracy on all 5, but keeps the encoder's 2,257 priming samples a gapless player trims | 610–2,440 | no panic | 13 releases |
| MP3 | `puremp3` 0.1.0 | MIT OR CC0-1.0 | ok | 0 | **fails**: RMS up to 1.2e-2 | 270–1,110 | **1 panic** | 1 release, 2019 |
| MP3 | `symphonia-bundle-mp3` 0.6.1 | **MPL-2.0** | **fails** | 0 | full accuracy | 1,070–4,800 | no panic | mature |
| Vorbis | `lewton` 0.10.2 | MIT OR Apache-2.0 | ok | 0 as built (7 only in its C-API module) | largest 3.7e-7, RMS 2.9e-8 | 465–790 | no panic | last release 2021-01-20 |
| Vorbis | `symphonia-codec-vorbis` 0.6.1 | **MPL-2.0** | **fails** | 0 | largest 1.8e-7 | 1,330–1,490 | no panic | mature |
| FLAC | `claxon` 0.4.3 | Apache-2.0 | ok | 5 (2 on the decode path) | bit-exact on all 3 | 880–1,220 | no panic | last release 2020-08-09 |
| FLAC | `rusty_flac` 0.2.1 | Apache-2.0 | ok | 11 | bit-exact on all 3 | 810–1,365 | no panic | 6 releases since 2026-08-13 |
| FLAC | `symphonia-bundle-flac` 0.6.1 | **MPL-2.0** | **fails** | 0 | bit-exact | 1,200–1,640 | no panic | mature |
| WAV | `hound` 3.5.1 | Apache-2.0 | ok | 2 (writer only) | bit-exact on all 4 | 1,150–2,340 | no panic | last release 2023-09-25 |
| WAV | `symphonia-format-riff` 0.6.1 | **MPL-2.0** | **fails** | 0 | bit-exact | 2,660–4,510 | no panic | mature |

`rusty_vorbis` 0.1.1 is an encoder only (no decoding API). `rmp3`, `minimp3`, `fdk-aac` and
`vorbis-sys` compile C. Peak memory decoding the largest file of each format: 15–17 MB (AAC),
37–71 MB (MP3), 106–113 MB (Vorbis), 117–141 MB (FLAC), 70–93 MB (WAV), mostly the whole file and
its decoded samples held in memory by the harness.

## 8. Questions for the maintainer (M15.R)

Each is a decision only the maintainer can take; the lead carries them to M15.R's check-in. No
task waits on them: the later tasks build on §2 and keep the tile path.

1. **HEVC and patents.** M10.R left HEVC still-image decoding (HEIC) out of release builds because
   decoding HEVC needs a patent licence in some countries (`formats.md` §3), and M14.R chose a
   pure-Rust HEVC video decoder. Do release builds carry `rusty_h265`, or does HEVC video sit
   behind the same non-default feature as HEIC (iPhone clips then stay tiles in official builds)?
   H.264 and AAC carry the same kind of question, in fewer countries (many of their patents have
   expired).
2. **`unsafe` in the passing video decoders.** H.264 passes only with `altc264`, whose decoding core
   uses 257 `unsafe` sites on hostile input (raw pointers into shared pictures, unchecked indexing,
   a lifetime `transmute` in its thread pool); it was first published 2026-10-14 and has one
   maintainer. HEVC passes the speed bar only with `rusty_h265`'s `simd` feature (702 `unsafe`
   sites of SIMD kernels; without it the whole decoder is `forbid(unsafe_code)` and runs at
   22–31 fps on 30 fps 1080p clips). Accept them as they are; or require the safe builds (H.264
   then has no passing crate and stays tiles, HEVC decodes below real time on one core and needs
   frame threading of our own); or accept them behind a fuzzing gate of our own before 1.0.
3. **The bar's frame rate.** The bar says "1080p in real time on one core". `rusty_h265` with
   `simd` meets it at 30 fps (51–56 fps) but not quite at 60 fps: the two iPhone 13 HDR clips,
   1080p60, decode at 48 and 59 fps; 4K60 at 22 fps. Is 1080p30 the bar (then HEVC passes as
   recorded, and the playback task adds threading for 60 fps and 4K), or 1080p60 (then HEVC passes only once
   Dalica threads it)? No openly licensed Android 4K H.264 original was found (about 1,700
   Internet Archive items probed; Wikimedia Commons takes no MP4 or MOV); 4K H.264 was measured on
   an iPhone 12 mini clip.
4. **Publishing the samples.** May the phone clips of §9 (CC BY-SA 4.0, CC0 and public domain;
   about 740 MB) go to `pkkid/dalica-corpus` as a `video` set, so the review's corpus job can rerun
   this measurement? The ITU conformance bitstreams carry no open licence and stay a download.
5. **The sound crates' age.** `lewton` (Vorbis) and `claxon` (FLAC) pass but have had no release
   since 2021 and 2020. Accept them, or prefer `rusty_flac` (new, 11 `unsafe`) for FLAC; Vorbis has
   no other allow-listed decoder (Symphonia is MPL-2.0 and would need a licence exception).

## 9. Samples

Fetched into the agent's scratch directory, never committed, not published (§8 asks). Phone files
from the Internet Archive (each item's licence as its uploader set it; `https://archive.org/download/<item>/<file>`):

| File | Item | Licence | What | SHA-256 |
|---|---|---|---|---|
| `IMG_0444.MOV` | `ankkumar-zurich-eprix-2018-02` | CC BY-SA 4.0 | iPhone 7 Plus, HEVC Main, 1080p30, landscape, 51 frames | `42c1f6d143e25f00e88ad8281a54c2e1d325e27afc6d73bb6eaaf16238d32a53` |
| `IMG_0445.MOV` | `ankkumar-zurich-eprix-2018-03` | CC BY-SA 4.0 | iPhone 7 Plus, HEVC Main, 1080p30, landscape, 706 frames; AAC-LC mono | `c4da19d500667740dafe4201d5a3245dbaa38a2cbb1793d345be46e6f2f2f3ef` |
| `IMG_0477.MOV` | `ankkumar-zurich-eprix-2018-05` | CC BY-SA 4.0 | iPhone 7 Plus, HEVC Main, 1080p30, portrait (rotation 90°), 628 frames | `1f1a7a969c1831e1d7b5fd545fb341d64cbb54854d8dc16b2742b82f9716eb86` |
| `IMG_0488.MOV` | `ankkumar-zurich-eprix-2018-11` | CC BY-SA 4.0 | iPhone 7 Plus, HEVC Main, 1080p30, portrait, 1,100 frames | `241f8f6af0a7e9ef7b233efe2c492256db863eb46000453c5ceb2948b9d816db` |
| `IMG_6851.MOV` | `img-6851_202505` | public domain mark | iPhone 13, H.264 High, 1080p29.97, portrait, 221 frames; AAC-LC stereo | `811d06bf464975c5131b8bcf6258d8cbc033b4c94e8986a0f636251df34842bb` |
| `20260418_0635.MOV` | `bo-ngua-xanh_20260418` | CC0 | iPhone 7 Plus, H.264 High, 1080p30, portrait, 2,492 frames | `e465cf1d5c9572d06f302a555356f6b4e53313f1fa3e319405edb04af13aa79c` |
| `Behind The Scene Mexico 2024 by L eau rend Guidali [Www.Etoile.App] 18.MOV` | `behind-the-scene-mexico-2024-by-l-eau-rend-guidali-www.-etoile.-app-18` | public domain mark | iPhone 12 mini, H.264 High, **4K 59.94**, portrait, 1,174 frames | `66e4127d06bd75bd707c2b7e566438eb27990e1049364effde80c2a8d9c81332` |
| `20260404_201346.mp4` | `20260404-201346` | CC0 | Android 14, H.264 High, 1080p30, portrait, 872 frames; AAC-LC 48 kHz stereo | `30316225ce021c1bb9a620700d24cfdc800871aec8ebc0031aa0e5191b4b096d` |
| `VID_20261017_160853309.mp4` | `outerpasta-videos-rog-lvl-20` | CC BY-SA 4.0 | Android 13, moto g play (2023), H.264 High, 1080p29.6, landscape, 484 frames | `34bde67731965ad0a085a63f7327d13af8b8a0ff322011fb0419c75358af715a` |
| `VID_20261017_141936777.mp4` | `outerpasta-videos-rog-lvl-20` | CC BY-SA 4.0 | the same phone, portrait, 172 frames | `7fc67fb2a16ccb59276f4ef90853050a2db5ff932f4ae22e7726fd8d00e744f3` |
| `M-6 Fun Fact.MOV` | `m-6-fun-fact` | CC0 | iPhone 13, HEVC Main 10, HLG/BT.2020, Dolby Vision profile 8.4, 1080p60, portrait, 2,167 frames | `6508907b38cc63e9a9d6af8868ec61a271a17a57962e622bbd561b975d43e326` |
| `104-107 Fun Fact.MOV` | `104-107-fun-fact` | CC0 | iPhone 13, the same format, 5,055 frames | `474d46d61bd7d7192cd8176b6ad39a281ce2661066f1349a2b74b9dbe001fb48` |

Made by the spike (CC0, from the clips above, with x265 4.1 through ffmpeg): `gen_1080_main10_hlg.hevc`
(from `IMG_0445.MOV`, 300 frames, Main 10, HLG/BT.2020 tags, B-frames;
`0a41ad0b0df0b6dc791484e4d496b787d304d48480db65aee98779658ac3de27`) and `gen_4k_main10_hlg.hevc`
(from the iPhone 12 mini clip, 240 frames, 2160 × 3840, Main 10 HLG at 40 Mbit/s;
`81255b3404093369e8d9735b90e48d4c06a01db2dc6f78545e753b2d9bd4eee8`): Main 10 with B-frames, and at
4K, which no public phone original was found for.

Conformance bitstreams (ITU, free to download, no open licence; not for publishing):
`https://www.itu.int/wftp3/av-arch/jctvc-site/bitstream_exchange/draft_conformance/HEVC_v1/` (all 147
zips, 124 MB) and `https://www.itu.int/wftp3/av-arch/jvt-site/draft_conformance/` (`AVCv1/` and
`FRExt/`, 177 zips); each zip's own published MD5 is the check. ISO/IEC 14496-26 AAC streams (ISO's
publicly available standards area,
`https://standards.iso.org/ittf/PubliclyAvailableStandards/ISO_IEC_14496-26_2010_Bitstreams/DVD1/mpeg4audio-conformance/compressedMp4/`):
`al00_44.mp4` (`27017a8c…f533f372`), `al02_44.mp4` (`1c00ddfd…`), `al04_44.mp4` (`e9838867…`),
`al05_48.mp4` (`b4447698…`), `al06_44.mp4` (`c7c36aa9…`, 3 channels), `al07_48.mp4` (`73554167…`,
5.1).

Music from Wikimedia Commons (`https://upload.wikimedia.org/wikipedia/commons/…`):

| File | Licence | What | SHA-256 |
|---|---|---|---|
| `Phil_Madison_-_Pull_Down_(1923,_Piano_by_Lionel_Belasco).mp3` (`e/e2/`) | public domain | MP3 44.1 kHz stereo, 193 s | `73798a2065b1e58c0ded7f4ddf11eb5198c0c9d374bcc36ebf409bc619d9b719` |
| `Amazing_Plan_(ISRC_USUAN1100737).mp3` (`6/67/`) | CC BY 3.0 | MP3 44.1 kHz stereo, 87 s | `76c8b73bd16b7c0437fcaf38b3503149426b3638b8b0fb6e731b330478ffe77d` |
| `Beethoven_-_Piano_Sonata_No._28_in_A_Major,_Op._101_-_I.…ogg` (`f/fe/`) | public domain | Vorbis 44.1 kHz stereo, 286 s | `f367c236b4e1bbbf1b93d2bb2697306a8dabd4aafc47589e70a5c69fd65fbf52` |
| `Luckey_Roberts_-_The_Music_Box_Rag_(1914).ogg` (`3/34/`) | public domain | Vorbis 48 kHz stereo, 184 s | `e29cf4b6759f2c8729a6a3e93c67c6134dae70573047849e30d96fa1109d417e` |
| `Kol_Nidre_-_cello_with_piano_played_by_Auguste_van_Biene_-_12inch_Zonophone_A_60.flac` (`b/ba/`) | public domain | FLAC 44.1 kHz stereo, 209 s | `55dcd0c516e8ef3b1c7366aa61b43a0c94ae3143bbee2c3375befc38984231ab` |
| `Louis_Chauvin_and_Scott_Joplin's_"Heliotrope_Bouquet"_(1907),_performed_by_John_Robson_on_piano_c._2007.flac` (`e/ed/`) | public domain | FLAC 44.1 kHz stereo, 267 s | `6066ce3827778bfa14f9ff43e7da3025f274f9d21bc56e36e1d104cfb23b7a95` |
| `Satie_Gymnopedie_No._3_for_piano_solo_01.wav` (`e/e1/`) | public domain | WAV 16-bit 44.1 kHz stereo, 30 s | `40b26ebc537d84180f7c13702152393a016347736a199d50e603b15f2676905f` |
| `Greensleeves_for_solo_piano.wav` (`8/83/`) | CC BY-SA 4.0 | WAV 16-bit 44.1 kHz stereo, 129 s | `06d57476655b0af6f64c5c36a5bcefb6f021c3992e338ae56b1faf689ddff9b5` |

Made by the spike from the Satie file (CC0): MP3 CBR 128 kbit/s, MP3 VBR (LAME `-q:a 2`), MP3 mono
22.05 kHz (MPEG-2 layer III), Vorbis q3, FLAC 24-bit, WAV 24-bit and 32-bit float.

