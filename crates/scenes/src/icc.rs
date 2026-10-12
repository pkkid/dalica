//! ICC display profiles built in code from published colorimetry, for tagging test files.
//!
//! Written from the public ICC.1 specification (v2.1 and v4.3 headers and tag types): a matrix/TRC
//! RGB profile (`desc`, `cprt`, `wtpt`, `rXYZ`/`gXYZ`/`bXYZ`, `rTRC`/`gTRC`/`bTRC`, and `chad` in v4)
//! or a grey profile (`kTRC`). The colorants come from each space's published primaries and white
//! point, adapted to the D50 profile connection space with the Bradford transform.
//!
//! - v4 profiles use parametric curves (`para`) and multi-localised text (`mluc`).
//! - v2 profiles use a 1024-entry `curv` table for piecewise curves (sRGB, Rec. 709), a single gamma
//!   `curv` otherwise, and `desc`/`text` for their text; `wtpt` holds the space's own white.
//!
//! Only `+ − × ÷` are used (powers go through a fixed Newton iteration rather than `powf`, whose
//! last bit may differ between platforms), so every profile has the same bytes everywhere.

/// A colour space with a published definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    /// IEC 61966-2-1: BT.709 primaries, D65, the sRGB curve.
    Srgb,
    /// DCI-P3 primaries with D65 and the sRGB curve.
    DisplayP3,
    /// Adobe RGB (1998): D65, gamma 563/256.
    AdobeRgb,
    /// ROMM RGB (ISO 22028-2): D50, gamma 1.8.
    ProPhoto,
    /// ITU-R BT.2020 primaries, D65, the BT.709/BT.2020 camera curve.
    Rec2020,
}

impl Space {
    pub const ALL: [Space; 5] = [Space::Srgb, Space::DisplayP3, Space::AdobeRgb, Space::ProPhoto, Space::Rec2020];

    /// The profile description.
    pub fn name(self) -> &'static str {
        match self {
            Space::Srgb => "sRGB",
            Space::DisplayP3 => "Display P3",
            Space::AdobeRgb => "Adobe RGB (1998) compatible",
            Space::ProPhoto => "ProPhoto (ROMM) RGB",
            Space::Rec2020 => "Rec. 2020",
        }
    }

    /// Chromaticities `(x, y)` of red, green, blue and white.
    fn chromaticities(self) -> [(f64, f64); 4] {
        const D65: (f64, f64) = (0.3127, 0.3290);
        match self {
            Space::Srgb => [(0.64, 0.33), (0.30, 0.60), (0.15, 0.06), D65],
            Space::DisplayP3 => [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060), D65],
            Space::AdobeRgb => [(0.64, 0.33), (0.21, 0.71), (0.15, 0.06), D65],
            Space::ProPhoto => [(0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001), (0.3457, 0.3585)],
            Space::Rec2020 => [(0.708, 0.292), (0.170, 0.797), (0.131, 0.046), D65],
        }
    }

    fn curve(self) -> Curve {
        match self {
            Space::Srgb | Space::DisplayP3 => Curve::Srgb,
            Space::AdobeRgb => Curve::Gamma(563, 256),
            Space::ProPhoto => Curve::Gamma(9, 5),
            Space::Rec2020 => Curve::Rec709,
        }
    }
}

/// The ICC version to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Version {
    /// 2.1.0
    V2,
    /// 4.3.0
    V4,
}

impl Version {
    pub const ALL: [Version; 2] = [Version::V2, Version::V4];
}

/// A tone curve: decoded = f(encoded).
#[derive(Clone, Copy, Debug)]
enum Curve {
    /// IEC 61966-2-1: `((x + 0.055) / 1.055)^2.4` above 0.04045, `x / 12.92` below.
    Srgb,
    /// BT.709 inverse: `((x + 0.099) / 1.099)^(1 / 0.45)` above 0.081, `x / 4.5` below.
    Rec709,
    /// `x^(n / d)`.
    Gamma(u32, u32),
}

impl Curve {
    fn eval(self, x: f64) -> f64 {
        match self {
            Curve::Srgb if x <= 0.04045 => x / 12.92,
            Curve::Srgb => pow_ratio((x + 0.055) / 1.055, 12, 5),
            Curve::Rec709 if x < 0.081 => x / 4.5,
            Curve::Rec709 => pow_ratio((x + 0.099) / 1.099, 20, 9),
            Curve::Gamma(n, d) => pow_ratio(x, n, d),
        }
    }

    /// The `para` function type and parameters.
    fn parametric(self) -> (u16, Vec<f64>) {
        match self {
            Curve::Srgb => (3, vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045]),
            Curve::Rec709 => (3, vec![1.0 / 0.45, 1.0 / 1.099, 0.099 / 1.099, 1.0 / 4.5, 0.081]),
            Curve::Gamma(n, d) => (0, vec![f64::from(n) / f64::from(d)]),
        }
    }
}

/// `base^(p / q)` for `base` in `[0, 1]`: the `q`-th root of `base^p` by Newton's method from 1,
/// which decreases monotonically to the root, with a fixed iteration cap (deterministic).
fn pow_ratio(base: f64, p: u32, q: u32) -> f64 {
    let base = base.clamp(0.0, 1.0);
    let a = (0..p).fold(1.0, |acc, _| acc * base);
    if a <= 0.0 || q == 0 {
        return a;
    }
    let qf = f64::from(q);
    let mut y = 1.0f64;
    for _ in 0..400 {
        let y_q1 = (1..q).fold(1.0, |acc, _| acc * y);
        let next = ((qf - 1.0) * y + a / y_q1) / qf;
        if next.is_nan() || next >= y {
            break;
        }
        y = next;
    }
    y
}

type Mat = [[f64; 3]; 3];
type Vec3 = [f64; 3];

/// The ICC PCS illuminant (D50) as the specification fixes it.
const PCS_D50: Vec3 = [0.9642, 1.0, 0.8249];

fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    m
}

fn apply(a: &Mat, v: Vec3) -> Vec3 {
    [0, 1, 2].map(|i| a[i][0] * v[0] + a[i][1] * v[1] + a[i][2] * v[2])
}

fn inverse(m: &Mat) -> Mat {
    let c = |r: usize, k: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (k1, k2) = ((k + 1) % 3, (k + 2) % 3);
        m[r1][k1] * m[r2][k2] - m[r1][k2] * m[r2][k1]
    };
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    let mut inv = [[0.0; 3]; 3];
    for (i, row) in inv.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            // The inverse is the transposed cofactor matrix over the determinant.
            *v = c(j, i) / det;
        }
    }
    inv
}

fn xyz((x, y): (f64, f64)) -> Vec3 {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// Bradford chromatic adaptation from white `from` to white `to`.
fn bradford(from: Vec3, to: Vec3) -> Mat {
    const B: Mat = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];
    let (s, d) = (apply(&B, from), apply(&B, to));
    let scale = [[d[0] / s[0], 0.0, 0.0], [0.0, d[1] / s[1], 0.0], [0.0, 0.0, d[2] / s[2]]];
    mul(&inverse(&B), &mul(&scale, &B))
}

/// The RGB → XYZ matrix of a space under its own white.
fn rgb_to_xyz(space: Space) -> (Mat, Vec3) {
    let [r, g, b, w] = space.chromaticities();
    let (r, g, b, white) = (xyz(r), xyz(g), xyz(b), xyz(w));
    let p = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let s = apply(&inverse(&p), white);
    (p.map(|row| [row[0] * s[0], row[1] * s[1], row[2] * s[2]]), white)
}

/// The D50-adapted colorants (columns red, green, blue) and the adaptation matrix of a space.
pub fn colorants(space: Space) -> ([Vec3; 3], [[f64; 3]; 3]) {
    let (m, white) = rgb_to_xyz(space);
    let chad = bradford(white, PCS_D50);
    let d50 = mul(&chad, &m);
    ([0, 1, 2].map(|k| [d50[0][k], d50[1][k], d50[2][k]]), chad)
}

/// A profile under construction: tags in the order they are added.
struct Builder {
    version: Version,
    tags: Vec<([u8; 4], Vec<u8>)>,
}

fn s15(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn type_head(sig: &[u8; 4]) -> Vec<u8> {
    let mut out = sig.to_vec();
    out.extend([0; 4]);
    out
}

impl Builder {
    fn new(version: Version) -> Builder {
        Builder { version, tags: Vec::new() }
    }

    fn tag(&mut self, sig: &[u8; 4], data: Vec<u8>) {
        self.tags.push((*sig, data));
    }

    fn xyz(&mut self, sig: &[u8; 4], v: Vec3) {
        let mut d = type_head(b"XYZ ");
        v.iter().for_each(|c| d.extend(s15(*c)));
        self.tag(sig, d);
    }

    fn text(&mut self, sig: &[u8; 4], text: &str) {
        let ascii: Vec<u8> = text.bytes().filter(|b| b.is_ascii() && *b != 0).collect();
        let d = match (self.version, sig) {
            (Version::V4, _) => {
                let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
                let mut d = type_head(b"mluc");
                d.extend(1u32.to_be_bytes());
                d.extend(12u32.to_be_bytes());
                d.extend(*b"enUS");
                d.extend((utf16.len() as u32).to_be_bytes());
                d.extend(28u32.to_be_bytes());
                d.extend(utf16);
                d
            }
            (Version::V2, b"desc") => {
                let mut d = type_head(b"desc");
                d.extend((ascii.len() as u32 + 1).to_be_bytes());
                d.extend(&ascii);
                d.push(0);
                // No Unicode or ScriptCode description: language 0, count 0; code 0, count 0, 67 bytes.
                d.extend([0; 8]);
                d.extend([0; 3]);
                d.extend([0; 67]);
                d
            }
            (Version::V2, _) => {
                let mut d = type_head(b"text");
                d.extend(&ascii);
                d.push(0);
                d
            }
        };
        self.tag(sig, d);
    }

    fn curve(&mut self, sig: &[u8; 4], curve: Curve) {
        let d = match (self.version, curve) {
            (Version::V4, _) => {
                let (kind, params) = curve.parametric();
                let mut d = type_head(b"para");
                d.extend(kind.to_be_bytes());
                d.extend([0; 2]);
                params.iter().for_each(|p| d.extend(s15(*p)));
                d
            }
            (Version::V2, Curve::Gamma(n, den)) => {
                // u8Fixed8Number: 563/256 is exact; 1.8 becomes 461/256.
                let gamma = ((f64::from(n) / f64::from(den)) * 256.0).round() as u16;
                let mut d = type_head(b"curv");
                d.extend(1u32.to_be_bytes());
                d.extend(gamma.to_be_bytes());
                d
            }
            (Version::V2, _) => {
                const N: u32 = 1024;
                let mut d = type_head(b"curv");
                d.extend(N.to_be_bytes());
                for i in 0..N {
                    let y = curve.eval(f64::from(i) / f64::from(N - 1));
                    d.extend(((y * 65535.0).round().clamp(0.0, 65535.0) as u16).to_be_bytes());
                }
                d
            }
        };
        self.tag(sig, d);
    }

    fn finish(self, colour_space: &[u8; 4]) -> Vec<u8> {
        let table_len = 4 + 12 * self.tags.len();
        let mut at = 128 + table_len;
        let mut table = (self.tags.len() as u32).to_be_bytes().to_vec();
        let mut data = Vec::new();
        for (sig, d) in &self.tags {
            let padded = d.len().next_multiple_of(4);
            table.extend(sig);
            table.extend((at as u32).to_be_bytes());
            table.extend((d.len() as u32).to_be_bytes());
            data.extend(d);
            data.resize(data.len() + padded - d.len(), 0);
            at += padded;
        }
        let mut out = Vec::with_capacity(at);
        out.extend((at as u32).to_be_bytes());
        out.extend([0; 4]); // preferred CMM
        out.extend(match self.version {
            Version::V2 => [2, 0x10, 0, 0],
            Version::V4 => [4, 0x30, 0, 0],
        });
        out.extend(*b"mntr");
        out.extend(colour_space);
        out.extend(*b"XYZ ");
        // Creation date: fixed, so the bytes never change (2026-01-01 00:00:00).
        [2026u16, 1, 1, 0, 0, 0].iter().for_each(|v| out.extend(v.to_be_bytes()));
        out.extend(*b"acsp");
        out.extend([0; 4 + 4 + 4 + 4 + 8]); // platform, flags, manufacturer, model, attributes
        out.extend(0u32.to_be_bytes()); // perceptual intent
        PCS_D50.iter().for_each(|c| out.extend(s15(*c)));
        out.extend([0; 4]); // creator
        out.extend([0; 16]); // profile ID: not computed
        out.extend([0; 28]);
        out.extend(table);
        out.extend(data);
        out
    }
}

const COPYRIGHT: &str = "No copyright, use freely (CC0). Dalica test profile.";

/// A matrix/TRC display profile of `space`.
pub fn rgb(space: Space, version: Version) -> Vec<u8> {
    let (cols, chad) = colorants(space);
    let (_, own_white) = rgb_to_xyz(space);
    let mut b = Builder::new(version);
    b.text(b"desc", &format!("{} (Dalica test, ICC {})", space.name(), if version == Version::V2 { 2 } else { 4 }));
    b.text(b"cprt", COPYRIGHT);
    match version {
        Version::V2 => b.xyz(b"wtpt", own_white),
        Version::V4 => {
            b.xyz(b"wtpt", PCS_D50);
            let mut d = type_head(b"sf32");
            chad.iter().flatten().for_each(|v| d.extend(s15(*v)));
            b.tag(b"chad", d);
        }
    }
    b.xyz(b"rXYZ", cols[0]);
    b.xyz(b"gXYZ", cols[1]);
    b.xyz(b"bXYZ", cols[2]);
    for sig in [b"rTRC", b"gTRC", b"bTRC"] {
        b.curve(sig, space.curve());
    }
    b.finish(b"RGB ")
}

/// A grey display profile: D65 white and the sRGB tone curve.
pub fn grey(version: Version) -> Vec<u8> {
    let d65 = xyz((0.3127, 0.3290));
    let mut b = Builder::new(version);
    b.text(b"desc", &format!("Grey, sRGB curve (Dalica test, ICC {})", if version == Version::V2 { 2 } else { 4 }));
    b.text(b"cprt", COPYRIGHT);
    match version {
        Version::V2 => b.xyz(b"wtpt", d65),
        Version::V4 => {
            b.xyz(b"wtpt", PCS_D50);
            let mut d = type_head(b"sf32");
            bradford(d65, PCS_D50).iter().flatten().for_each(|v| d.extend(s15(*v)));
            b.tag(b"chad", d);
        }
    }
    b.curve(b"kTRC", Curve::Srgb);
    b.finish(b"GRAY")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powers_match_the_closed_forms() {
        for (b, p, q) in [(0.5, 12, 5), (0.25, 2, 1), (1.0, 20, 9), (0.0, 12, 5), (0.9, 9, 5), (0.04, 20, 9), (0.0789, 12, 5)] {
            let want = f64::powf(b, f64::from(p) / f64::from(q));
            assert!((pow_ratio(b, p, q) - want).abs() < 1e-12, "{b}^({p}/{q}) = {} vs {want}", pow_ratio(b, p, q));
        }
    }

    #[test]
    fn srgb_colorants_match_the_published_d50_values() {
        // The D50 colorants of IEC 61966-2-1 as ICC profiles carry them (to four decimals).
        let (c, _) = colorants(Space::Srgb);
        let want = [[0.4361, 0.2225, 0.0139], [0.3851, 0.7169, 0.0971], [0.1431, 0.0606, 0.7141]];
        for (got, want) in c.iter().flatten().zip(want.iter().flatten()) {
            assert!((got - want).abs() < 2e-4, "{got} vs {want}");
        }
        // The colorants add up to the PCS white.
        for k in 0..3 {
            assert!((c[0][k] + c[1][k] + c[2][k] - PCS_D50[k]).abs() < 1e-9);
        }
    }

    #[test]
    fn headers_and_tag_tables_are_well_formed() {
        for version in Version::ALL {
            for p in Space::ALL.map(|s| rgb(s, version)).into_iter().chain([grey(version)]) {
                assert_eq!(u32::from_be_bytes([p[0], p[1], p[2], p[3]]) as usize, p.len());
                assert_eq!(p.len() % 4, 0);
                assert_eq!(&p[36..40], b"acsp");
                let n = u32::from_be_bytes([p[128], p[129], p[130], p[131]]) as usize;
                for k in 0..n {
                    let e = 132 + 12 * k;
                    let off = u32::from_be_bytes([p[e + 4], p[e + 5], p[e + 6], p[e + 7]]) as usize;
                    let len = u32::from_be_bytes([p[e + 8], p[e + 9], p[e + 10], p[e + 11]]) as usize;
                    assert_eq!(off % 4, 0);
                    assert!(off + len <= p.len());
                }
            }
        }
    }
}
