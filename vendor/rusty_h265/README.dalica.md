# rusty_h265 0.6.2, vendored by Dalica

This directory is `rusty_h265` 0.6.2 from crates.io (Apache-2.0, `LICENSE`; upstream
<https://github.com/Remade-With-Rust/rusty_h265>, by Mata Network), `.crate` SHA-256
`56850745172ccec10fdb3829ae06b5b6b1bc6995571dd59373e5ef7bc9007d7d`, with the changes below.
Dalica uses it through `[patch.crates-io]` in the workspace's `Cargo.toml`, behind
`dalica-image`'s `hevc` feature (`docs/video-decoders.md`). Its kernels stay in the published
`rusty_h265-accel` =0.6.2, unchanged.

## Why

The playback bar (`docs/video-decoders.md` §1, roadmap M16.14): a 1080p30 clip's frame decoded
within 16 ms at p95. The published decoder has no threads, and one iPhone 1080p HEVC picture
takes 18 to 21 ms on one core. The maintainer's answer on M16.14's issue (2026-11-06): thread
the decoder. No newer release has threads, and decoding key-frame groups in parallel in Dalica's
own code cannot bring the first group's frames (and B-frame reordering's bursts) under the bar,
so the threads are here.

## What changed

Added (Dalica's own code, MIT OR Apache-2.0 like the rest of Dalica, offered here under
Apache-2.0 as the crate is):

- `src/wpp.rs`: a picture whose one slice segment carries a wavefront substream per CTB row
  (`entropy_coding_sync_enabled_flag`, as phone encoders write them) decodes its rows on several
  threads. Each thread decodes its rows into a private picture, receives the row above's last
  sample row and 4x4 maps and its stored contexts through a channel, and copies each finished row
  into its own band of the real picture. A failing row or an early slice end stops the rows below
  it, and the CTBs after the first such point are put back as never decoded, as the serial loop
  leaves them.
- `src/filters/par.rs`: deblocking and SAO on the same threads, by bands of CTB rows that no
  edge's footprint crosses.

Changed (each file says so at its top):

- `src/ctu.rs`: `SliceDecoder::decode_row` (one row on its own) and `target`; `RowEnd`.
- `src/decoder.rs`: `Decoder::set_threads`; a picture that qualifies goes to `wpp.rs`; the
  filters get the thread count; `FilterScratch::sao_par`.
- `src/filters.rs`: `apply_in_loop_filters` takes the thread count and hands the work to
  `filters/par.rs` above one.
- `src/lib.rs`: the `wpp` module.
- `Cargo.toml`: written for this copy (no binary, tests or `bench-alloc`; the accel crate pinned).
- Removed: the conformance harness binary (`src/bin`) and the conformance tests (`tests/`),
  which need the JCT-VC vectors.

No `unsafe` was added: the crate keeps `#![forbid(unsafe_code)]`; threads share nothing mutably
(`std::thread::scope`, channels, disjoint `chunks_mut`). With one thread (`set_threads(1)`, the
default) the published code paths run unchanged.

## How it was checked

The output is the serial decoder's, bit for bit, for every thread count: the decoded YUV of
every HEVC phone clip in the corpus (`corpus/video`) and of x265 streams covering CTB sizes 16,
32 and 64, 8 and 10 bits, sizes that are not whole CTBs, and B-frames, compared at 1, 2, 3, 8
and 16 threads (`crates/image/tests/video_frames.rs` keeps a small one of these). Numbers are in
`docs/video-decoders.md` §1.

## Updating

Moving to a newer upstream release means re-applying these changes to it (or dropping them if
upstream threads its decoder), comparing output as above, and updating the SHA-256 here.
