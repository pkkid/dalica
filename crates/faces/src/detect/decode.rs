//! The `yunet-v2` decoder: the network's per-cell scores, boxes and landmarks turned into faces
//! normalised on the photo, then non-maximum suppression.
//!
//! For each stride (8, 16, 32) the network gives, for every cell of its grid, a class score
//! `cls_<s>`, an objectness `obj_<s>`, a box `bbox_<s>` (centre offset in cells, then the log of
//! the width and height in cells) and five landmarks `kps_<s>` (offsets in cells). A cell's score
//! is `√(cls · obj)`.
//!
//! Ported from LightCraft `crates/faces/src/yunet.rs` @ f34c7a8a (MIT OR Apache-2.0).

use crate::net::Tensor;

use super::{DetectError, Face, Options};

/// The grids' strides.
pub(crate) const STRIDES: [usize; 3] = [8, 16, 32];

/// The outputs of one stride and how many numbers each gives per cell.
pub(crate) const PARTS: [(&str, usize); 4] = [("cls", 1), ("obj", 1), ("bbox", 4), ("kps", 10)];

/// Candidates kept before suppression, so a hostile or noisy output cannot make it slow.
const MAX_CANDIDATES: usize = 4000;

/// How the network's input maps back to the photo: input pixels per photo pixel, and the photo's
/// size in pixels.
pub(crate) struct Frame {
    pub side: usize,
    pub scale: f32,
    pub width: f32,
    pub height: f32,
}

fn output<'a>(outputs: &'a [(String, Tensor)], name: &str) -> Result<&'a Tensor, DetectError> {
    outputs.iter().find(|(n, _)| n == name).map(|(_, t)| t).ok_or_else(|| DetectError::Model(format!("the model has no output `{name}`")))
}

/// Every cell at or above the score threshold, as a face normalised on the photo (not yet clipped).
pub(crate) fn candidates(outputs: &[(String, Tensor)], frame: &Frame, opts: &Options) -> Result<Vec<Face>, DetectError> {
    let mut found: Vec<Face> = Vec::new();
    let (fx, fy) = (frame.scale * frame.width, frame.scale * frame.height);
    for stride in STRIDES {
        let cls = output(outputs, &format!("cls_{stride}"))?;
        let obj = output(outputs, &format!("obj_{stride}"))?;
        let bbox = output(outputs, &format!("bbox_{stride}"))?;
        let kps = output(outputs, &format!("kps_{stride}"))?;
        let cols = frame.side / stride;
        let cells = cols * cols;
        if cls.data.len() != cells || obj.data.len() != cells || bbox.data.len() != cells * 4 || kps.data.len() != cells * 10 {
            return Err(DetectError::Model(format!("the outputs for stride {stride} have the wrong size")));
        }
        let s = stride as f32;
        for (i, (c, o)) in cls.data.iter().zip(&obj.data).enumerate() {
            let score = (c.clamp(0.0, 1.0) * o.clamp(0.0, 1.0)).sqrt();
            if score.is_nan() || score < opts.score || score <= 0.0 {
                continue;
            }
            let (Some(&[bx, by, bw, bh]), Some(k)) = (bbox.data.get(i * 4..i * 4 + 4), kps.data.get(i * 10..i * 10 + 10)) else {
                continue;
            };
            let (row, col) = ((i / cols) as f32, (i % cols) as f32);
            let (cx, cy) = ((col + bx) * s, (row + by) * s);
            let (w, h) = (bw.clamp(-10.0, 10.0).exp() * s, bh.clamp(-10.0, 10.0).exp() * s);
            let mut landmarks = [(0.0, 0.0); 5];
            for (l, &[lx, ly]) in landmarks.iter_mut().zip(k.as_chunks::<2>().0) {
                *l = ((lx + col) * s / fx, (ly + row) * s / fy);
            }
            let face = Face { rect: [(cx - w / 2.0) / fx, (cy - h / 2.0) / fy, (cx + w / 2.0) / fx, (cy + h / 2.0) / fy], score, landmarks };
            if face.rect.iter().all(|v| v.is_finite()) && face.landmarks.iter().all(|(x, y)| x.is_finite() && y.is_finite()) {
                found.push(face);
            }
        }
    }
    Ok(found)
}

/// The best faces, clipped to the photo, none overlapping a better one by more than
/// `opts.nms_iou`, at most `opts.max_faces`. Ties keep the grids' order, so the result is the same
/// on every run.
pub(crate) fn suppress(mut found: Vec<Face>, opts: &Options) -> Vec<Face> {
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    found.truncate(MAX_CANDIDATES);
    let mut kept: Vec<Face> = Vec::new();
    for f in found {
        if kept.len() >= opts.max_faces {
            break;
        }
        let [x0, y0, x1, y1] = f.rect.map(|v| v.clamp(0.0, 1.0));
        let clipped = Face { rect: [x0, y0, x1, y1], ..f };
        if !(x1 - x0 > 1e-4 && y1 - y0 > 1e-4) || kept.iter().any(|k| iou(&k.rect, &clipped.rect) > opts.nms_iou) {
            continue;
        }
        kept.push(clipped);
    }
    kept
}

/// Intersection over union of two `[x0, y0, x1, y1]` boxes.
pub fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = w * h;
    let union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter;
    if union > 0.0 { inter / union } else { 0.0 }
}
