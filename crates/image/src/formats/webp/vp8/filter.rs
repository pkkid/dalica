//! The loop filters (RFC 6386 §15) on the eight pixels across one edge, `p3 p2 p1 p0 | q0 q1 q2
//! q3`: the caller gathers them from a row or a column and puts them back, so every index here is
//! a constant into an 8-pixel array.
//!
//! Adapted from `image-webp` 0.2.4 `src/loop_filter.rs` (github.com/image-rs/image-webp, MIT OR
//! Apache-2.0; `NOTICE`): its `*_horizontal` functions, which already take the eight pixels in
//! this order, on an array instead of a slice.

fn c(v: i32) -> i32 {
    v.clamp(-128, 127)
}

fn u2s(v: u8) -> i32 {
    i32::from(v) - 128
}

fn s2u(v: i32) -> u8 {
    (c(v) + 128) as u8
}

fn diff(a: u8, b: u8) -> u8 {
    a.abs_diff(b)
}

/// The two middle pixels' common adjustment (§15.2); returns the filter value `a`.
fn common_adjust(use_outer_taps: bool, px: &mut [u8; 8]) -> i32 {
    let (p1, p0, q0, q1) = (u2s(px[2]), u2s(px[3]), u2s(px[4]), u2s(px[5]));
    let outer = if use_outer_taps { c(p1 - q1) } else { 0 };
    let a = c(outer + 3 * (q0 - p0));
    let b = c(a + 3) >> 3;
    let a = c(a + 4) >> 3;
    px[4] = s2u(q0 - a);
    px[3] = s2u(p0 + b);
    a
}

fn simple_threshold(limit: i32, px: &[u8; 8]) -> bool {
    i32::from(diff(px[3], px[4])) * 2 + i32::from(diff(px[2], px[5])) / 2 <= limit
}

fn should_filter(interior: u8, edge: u8, px: &[u8; 8]) -> bool {
    simple_threshold(i32::from(edge), px)
        && diff(px[0], px[1]) <= interior
        && diff(px[1], px[2]) <= interior
        && diff(px[2], px[3]) <= interior
        && diff(px[7], px[6]) <= interior
        && diff(px[6], px[5]) <= interior
        && diff(px[5], px[4]) <= interior
}

fn high_edge_variance(threshold: u8, px: &[u8; 8]) -> bool {
    diff(px[2], px[3]) > threshold || diff(px[5], px[4]) > threshold
}

/// The simple filter (§15.2): at most the two pixels next to the edge change.
pub(super) fn simple(edge: u8, px: &mut [u8; 8]) {
    if simple_threshold(i32::from(edge), px) {
        common_adjust(true, px);
    }
}

/// The normal filter on an edge between subblocks (§15.3).
pub(super) fn subblock(hev: u8, interior: u8, edge: u8, px: &mut [u8; 8]) {
    if should_filter(interior, edge, px) {
        let hv = high_edge_variance(hev, px);
        let a = (common_adjust(hv, px) + 1) >> 1;
        if !hv {
            px[5] = s2u(u2s(px[5]) - a);
            px[2] = s2u(u2s(px[2]) + a);
        }
    }
}

/// The normal filter on an edge between macroblocks (§15.3): up to three pixels each side.
pub(super) fn macroblock(hev: u8, interior: u8, edge: u8, px: &mut [u8; 8]) {
    if !should_filter(interior, edge, px) {
        return;
    }
    if high_edge_variance(hev, px) {
        common_adjust(true, px);
        return;
    }
    let (p2, p1, p0) = (u2s(px[1]), u2s(px[2]), u2s(px[3]));
    let (q0, q1, q2) = (u2s(px[4]), u2s(px[5]), u2s(px[6]));
    let w = c(c(p1 - q1) + 3 * (q0 - p0));
    let a = c((27 * w + 63) >> 7);
    px[4] = s2u(q0 - a);
    px[3] = s2u(p0 + a);
    let a = c((18 * w + 63) >> 7);
    px[5] = s2u(q1 - a);
    px[2] = s2u(p1 + a);
    let a = c((9 * w + 63) >> 7);
    px[6] = s2u(q2 - a);
    px[1] = s2u(p2 + a);
}
