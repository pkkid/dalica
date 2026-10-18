//! The edit-operation catalogue (`mcp.md` §4.2 `list_edit_ops`, `data-model.md` §3.1, `editing.md`
//! §6–§8, §11, §13): one row per operation the parser knows, with its English label, its
//! parameters (type, range, default), whether it may change the picture's size, the milestone that
//! brings it, and whether this build renders it; an effect's row also says where its tile stands,
//! whether it applies in one click, its crosshair and its tooltip ([`Effect`], `editing.md` §8.1).
//! Agents read it to build recipes, the engine lists the effect tabs from it (`effect.list`) and
//! the panel builds its controls from it; the tests check it against the parser and against each
//! module's clamps, so the rows cannot drift from what Dalica does.
//!
//! - The rows follow the parser: `rotate`, `flip_h`, `flip_v` and every name of [`crate::LISTED`],
//!   in that order, so the 27 effects stand in the order of their tabs' grids.
//! - Ranges and defaults are the module's own constants (the effects' too, M12.1), so a row says
//!   what the module clamps to, and every default is settled.
//! - [`OpInfo::renders`] asks the operation's module, so a row turns on with its renderer.

use crate::ops::{self, canvas, colour, detail, focal, looks, mats, remap, retouch, soften, straighten, stylize, tone};

/// What one parameter holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A number brought into `min..=max` when read (a module clamps rather than refuses).
    Number { min: f64, max: f64 },
    /// A number whose range a module has yet to settle. No row has one since M12.1 settled the
    /// effects'; kept so readers of the catalogue keep a case for it.
    Open,
    /// One of these words, exactly.
    Choice(&'static [&'static str]),
    /// An sRGB colour `#rrggbb`; with `none`, also the word `none` (no colour).
    Colour { none: bool },
    /// A checkbox: `true` or `false`.
    Flag,
    /// A time in seconds from the start of a video, 0 to its length.
    Seconds,
    /// A list of strokes separated by `;`, at most `max`, each holding `fields` separated by `,`.
    /// It is always the last parameter: the strokes follow the other parameters after a `;`
    /// (`soften(50,50;paint,0.05,0.5,0.5)`), or make up the whole text when there are none
    /// (`retouch(…;…)`). No strokes (`retouch()`, `soften(50,50)`) is the default. A Soften
    /// stroke's last two fields (a point) repeat, once per point of the stroke.
    Strokes { max: usize, fields: &'static [Param] },
}

/// One parameter of an operation, in the order the entry writes them, separated by `,`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Param {
    /// The parameter's name in the syntax (`x0`, `fill`, `deg`); the effect panel's controls are
    /// named after it (`slider:effect.<name>`).
    pub name: &'static str,
    /// The English control label (`editing.md`), or a short description where no control shows it.
    pub label: &'static str,
    pub kind: Kind,
    /// The default as the entry writes it; empty only for a field with no default (a crop of a
    /// photo has its own; a trim's times, a stroke's fields).
    pub default: &'static str,
    /// Whether the number's slider is logarithmic (Glow's Radius).
    pub log: bool,
}

/// The milestone that brought (or brings) an operation.
pub type Milestone = &'static str;

/// The crosshair an effect shows on the photo while its panel is open (`editing.md` §8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crosshair {
    /// None.
    None,
    /// A centre (`x`, `y`) with the sharp circle of radius `size` drawn around it (Soft Focus,
    /// Focal B&W, Focal Zoom).
    Circle,
    /// A start (`x`, `y`) and a direction (`angle`) set by dragging from it (Graduated Tint).
    Gradient,
}

/// Where an effect's tile stands and how it behaves (`editing.md` §8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    /// The edit panel's tab: 3, 4 or 5.
    pub tab: u8,
    /// The tile's place in the tab's grid, 0-based, row by row three to a row.
    pub place: u8,
    /// Applies at once from its tile (with the "1-click" badge); otherwise the tile opens the
    /// effect's panel.
    pub one_click: bool,
    pub crosshair: Crosshair,
    /// The tile's tooltip, in English; an effect that changes the picture's size says so.
    pub tooltip: &'static str,
}

/// The effect tabs, with the most tiles each holds (three columns, at most four rows).
pub const TABS: [u8; 3] = [3, 4, 5];
/// The most tiles in one effect tab.
pub const MAX_TILES: usize = 12;

/// One operation of the catalogue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpInfo {
    /// The name in the recipe (`crop`, `tuning`).
    pub name: &'static str,
    /// The English label, the one "Undo …" names (`editing.md` §5.1).
    pub label: &'static str,
    /// The syntax, as `data-model.md` §3.1 writes it (`crop(x0,y0,x1,y1)`).
    pub syntax: &'static str,
    pub params: &'static [Param],
    /// Whether the operation may change the picture's width or height (a crop; a quarter turn; a
    /// frame).
    pub changes_size: bool,
    pub milestone: Milestone,
    /// For the 27 effects, their tile (`None` for every other operation).
    pub effect: Option<Effect>,
    /// What else an agent should know (the rules of `data-model.md` §3.1, the units of sizes).
    pub note: &'static str,
}

impl OpInfo {
    /// Whether this build renders the operation (its module's version is 1 or more); an operation
    /// that does not render is kept in the recipe and skipped.
    pub fn renders(&self) -> bool {
        ops::version(self.name).is_some_and(|v| v > 0)
    }

    /// Whether the operation has a module that reads its parameters (and so refuses bad ones).
    pub fn typed(&self) -> bool {
        ops::Typed::parse(self.name, "").is_some()
    }

    /// The entry at its defaults (`crop(0,0,1,1)`, `autofix()`, `soften(50,50)`), when every
    /// default is settled.
    pub fn default_entry(&self) -> Option<String> {
        let mut fields = Vec::with_capacity(self.params.len());
        for p in self.params {
            match p.kind {
                // No strokes: nothing to write.
                Kind::Strokes { .. } => {}
                _ if p.default.is_empty() => return None,
                _ => fields.push(p.default),
            }
        }
        Some(format!("{}({})", self.name, fields.join(",")))
    }
}

/// Every operation the parser knows, in its order: the rotations and flips, then [`crate::LISTED`].
pub fn catalog() -> &'static [OpInfo] {
    &CATALOG
}

/// The row of operation `name`.
pub fn lookup(name: &str) -> Option<&'static OpInfo> {
    CATALOG.iter().find(|op| op.name == name)
}

/// The effects of tab `tab` (3, 4 or 5) in their grid's order; none for any other tab.
pub fn effects(tab: u8) -> impl Iterator<Item = (&'static OpInfo, Effect)> {
    CATALOG.iter().filter_map(move |op| op.effect.filter(|e| e.tab == tab).map(|e| (op, e)))
}

const fn number(name: &'static str, label: &'static str, range: (f64, f64), default: &'static str) -> Param {
    Param { name, label, kind: Kind::Number { min: range.0, max: range.1 }, default, log: false }
}

const fn colour(name: &'static str, label: &'static str, default: &'static str) -> Param {
    Param { name, label, kind: Kind::Colour { none: false }, default, log: false }
}

const fn flag(name: &'static str, label: &'static str, default: &'static str) -> Param {
    Param { name, label, kind: Kind::Flag, default, log: false }
}

const fn fade(default: &'static str) -> Param {
    number("fade", "Fade", ops::FADE, default)
}

const UNIT: (f64, f64) = ops::UNIT;
/// How far outside the picture a retouch point may lie (the module's clamp).
const STROKE_POINT: (f64, f64) = (-1.0, 2.0);

const STROKE: [Param; 5] = [
    number("sx", "Source x", STROKE_POINT, ""),
    number("sy", "Source y", STROKE_POINT, ""),
    number("tx", "Target x", STROKE_POINT, ""),
    number("ty", "Target y", STROKE_POINT, ""),
    number("r", "Radius (of the shorter side)", (retouch::MIN_RADIUS, retouch::MAX_RADIUS), ""),
];

const BRUSH: [Param; 4] = [
    Param { name: "mode", label: "Paint or Erase", kind: Kind::Choice(&["paint", "erase"]), default: "", log: false },
    number("r", "Brush radius (of the shorter side)", soften::RADIUS, ""),
    number("x", "Point x", soften::POINT, ""),
    number("y", "Point y", soften::POINT, ""),
];

const NONE: &[Param] = &[];

/// An effect's row.
#[allow(clippy::too_many_arguments, reason = "one row of a table, every field named at the call")]
const fn fx(
    name: &'static str,
    label: &'static str,
    syntax: &'static str,
    params: &'static [Param],
    changes_size: bool,
    (tab, place): (u8, u8),
    one_click: bool,
    crosshair: Crosshair,
    tooltip: &'static str,
    note: &'static str,
) -> OpInfo {
    OpInfo { name, label, syntax, params, changes_size, milestone: "M12", effect: Some(Effect { tab, place, one_click, crosshair, tooltip }), note }
}

const SIZE_NOTE: &str = "Changes the picture's size: later effects work on the larger picture, a later crop still crops the photo.";

static CATALOG: [OpInfo; 39] = [
    OpInfo {
        name: "rotate",
        label: "Rotate",
        syntax: "rotate(deg)",
        params: &[Param { name: "deg", label: "Degrees clockwise", kind: Kind::Choice(&["90", "180", "270"]), default: "90", log: false }],
        changes_size: true,
        milestone: "M6",
        effect: None,
        note: "A rotation right after a rotation merges into one entry; a whole turn removes it. A quarter turn swaps width and height.",
    },
    OpInfo {
        name: "flip_h",
        label: "Flip Horizontal",
        syntax: "flip_h()",
        params: NONE,
        changes_size: false,
        milestone: "M6",
        effect: None,
        note: "Mirrors left to right; the same flip twice in a row cancels.",
    },
    OpInfo {
        name: "flip_v",
        label: "Flip Vertical",
        syntax: "flip_v()",
        params: NONE,
        changes_size: false,
        milestone: "M6",
        effect: None,
        note: "Mirrors top to bottom; the same flip twice in a row cancels.",
    },
    OpInfo {
        name: "crop",
        label: "Crop",
        syntax: "crop(x0,y0,x1,y1)",
        params: &[
            number("x0", "Left", UNIT, "0"),
            number("y0", "Top", UNIT, "0"),
            number("x1", "Right", UNIT, "1"),
            number("y1", "Bottom", UNIT, "1"),
        ],
        changes_size: true,
        milestone: "M7",
        effect: None,
        note: "A normalised rectangle of the straightened photo in the stored frame; a reversed or empty one is refused. Only the last crop counts.",
    },
    OpInfo {
        name: "straighten",
        label: "Straighten",
        syntax: "straighten(deg)",
        params: &[number("deg", "Degrees clockwise", (-straighten::MAX_DEGREES, straighten::MAX_DEGREES), "0")],
        changes_size: false,
        milestone: "M7",
        effect: None,
        note: "Turned about the centre and enlarged so no empty corner shows. Only the last straighten counts.",
    },
    OpInfo {
        name: "retouch",
        label: "Retouch",
        syntax: "retouch(sx,sy,tx,ty,r;…)",
        params: &[Param {
            name: "strokes",
            label: "Patches",
            kind: Kind::Strokes { max: retouch::MAX_STROKES, fields: &STROKE },
            default: "",
            log: false,
        }],
        changes_size: false,
        milestone: "M7",
        effect: None,
        note: "Each stroke heals the patch at the target from the source, in order; points in the stored frame, the radius relative to the shorter side.",
    },
    OpInfo {
        name: "autofix",
        label: "Auto Fix",
        syntax: "autofix()",
        params: NONE,
        changes_size: false,
        milestone: "M7",
        effect: None,
        note: "Auto Color, then Auto Contrast, then a little Fill Light, measured on the picture at its place in the recipe.",
    },
    OpInfo {
        name: "autocontrast",
        label: "Auto Contrast",
        syntax: "autocontrast()",
        params: NONE,
        changes_size: false,
        milestone: "M7",
        effect: None,
        note: "Black and white points and midtones, linked across the channels.",
    },
    OpInfo {
        name: "autocolor",
        label: "Auto Color",
        syntax: "autocolor()",
        params: NONE,
        changes_size: false,
        milestone: "M7",
        effect: None,
        note: "Removes a colour cast.",
    },
    OpInfo {
        name: "fill_light",
        label: "Fill Light",
        syntax: "fill_light(a)",
        params: &[number("a", "Fill Light", tone::FILL, "0")],
        changes_size: false,
        milestone: "M7",
        effect: None,
        note: "Older recipes' Fill Light: rendered as tuning(a,0,0,0,none) and kept as written. Dalica writes tuning.",
    },
    OpInfo {
        name: "tuning",
        label: "Tuning",
        syntax: "tuning(fill,highlights,shadows,temp,neutral)",
        params: &[
            number("fill", "Fill Light", tone::FILL, "0"),
            number("highlights", "Highlights", tone::HIGHLIGHTS, "0"),
            number("shadows", "Shadows", tone::SHADOWS, "0"),
            number("temp", "Color Temperature", tone::TEMP, "0"),
            Param { name: "neutral", label: "Neutral Color", kind: Kind::Colour { none: true }, default: "none", log: false },
        ],
        changes_size: false,
        milestone: "M7",
        effect: None,
        note: "The whole Tuning tab as one entry; the defaults change nothing.",
    },
    OpInfo {
        name: "trim",
        label: "Trim",
        syntax: "trim(start,end)",
        params: &[
            Param { name: "start", label: "Start", kind: Kind::Seconds, default: "", log: false },
            Param { name: "end", label: "End", kind: Kind::Seconds, default: "", log: false },
        ],
        changes_size: false,
        milestone: "M15",
        effect: None,
        note: "Videos only.",
    },
    // Tab 3, Effects (editing.md §8.2).
    fx(
        "sharpen",
        "Sharpen",
        "sharpen(a)",
        &[number("a", "Amount", detail::SHARPEN, "0.6")],
        false,
        (3, 0),
        false,
        Crosshair::None,
        "Sharpens detail and edges.",
        "",
    ),
    fx("sepia", "Sepia", "sepia()", NONE, false, (3, 1), true, Crosshair::None, "Turns the photo warm brown, like an old print.", "One click."),
    fx(
        "bw",
        "B&W",
        "bw()",
        NONE,
        false,
        (3, 2),
        true,
        Crosshair::None,
        "Turns the photo black and white.",
        "One click; Ctrl+Shift+B in the edit view.",
    ),
    fx(
        "warmify",
        "Warmify",
        "warmify()",
        NONE,
        false,
        (3, 3),
        true,
        Crosshair::None,
        "Warms the colours and gives skin a golden tone.",
        "One click.",
    ),
    fx(
        "grain",
        "Film Grain",
        "grain(grain,lighten)",
        &[number("grain", "Grain", detail::GRAIN, "10"), flag("lighten", "Lighten", "false")],
        false,
        (3, 4),
        false,
        Crosshair::None,
        "Adds the grain of film.",
        "",
    ),
    fx(
        "tint",
        "Tint",
        "tint(color,fade)",
        &[colour("color", "Tint color", "#80cfff"), fade("0")],
        false,
        (3, 5),
        false,
        Crosshair::None,
        "Takes the colour out, then tints the photo with one colour.",
        "",
    ),
    fx(
        "saturation",
        "Saturation",
        "saturation(a)",
        &[number("a", "Amount", colour::SATURATION, "0.16")],
        false,
        (3, 6),
        false,
        Crosshair::None,
        "Makes the colours richer or paler.",
        "-1 is black and white, 0 the photo.",
    ),
    fx(
        "soft_focus",
        "Soft Focus",
        "soft_focus(x,y,size,amount)",
        &[
            number("x", "Centre x", UNIT, "0.5"),
            number("y", "Centre y", UNIT, "0.5"),
            number("size", "Size", focal::SIZE, "0.3"),
            number("amount", "Amount", focal::AMOUNT, "0.5"),
        ],
        false,
        (3, 7),
        false,
        Crosshair::Circle,
        "Blurs the photo outside a sharp circle.",
        "The centre is a point of the stored frame; Size is the circle's radius as a fraction of the shorter side.",
    ),
    fx(
        "glow",
        "Glow",
        "glow(i,r)",
        &[number("i", "Intensity", detail::GLOW_INTENSITY, "0.65"), Param { log: true, ..number("r", "Radius", detail::GLOW_RADIUS, "3") }],
        false,
        (3, 8),
        false,
        Crosshair::None,
        "Makes the bright areas glow.",
        "Radius in percent of the shorter side, on a logarithmic slider.",
    ),
    fx(
        "filtered_bw",
        "Filtered B&W",
        "filtered_bw(color)",
        &[colour("color", "Filter color", "#ff8000")],
        false,
        (3, 9),
        false,
        Crosshair::None,
        "Black and white as if shot through a coloured filter.",
        "",
    ),
    fx(
        "focal_bw",
        "Focal B&W",
        "focal_bw(x,y,size,sharpness)",
        &[
            number("x", "Centre x", UNIT, "0.5"),
            number("y", "Centre y", UNIT, "0.5"),
            number("size", "Size", focal::SIZE, "0.3"),
            number("sharpness", "Sharpness", focal::SHARPNESS, "0.5"),
        ],
        false,
        (3, 10),
        false,
        Crosshair::Circle,
        "Keeps the colour inside a circle and turns the rest black and white.",
        "The centre is a point of the stored frame; Size is the circle's radius as a fraction of the shorter side.",
    ),
    fx(
        "graduated_tint",
        "Graduated Tint",
        "graduated_tint(x,y,angle,feather,shade,color)",
        &[
            number("x", "Start x", UNIT, "0.5"),
            number("y", "Start y", UNIT, "0.5"),
            number("angle", "Angle", focal::ANGLE, "0"),
            number("feather", "Feather", focal::FEATHER, "0.25"),
            number("shade", "Shade", focal::SHADE, "0.25"),
            colour("color", "Color", "#9cc8ff"),
        ],
        false,
        (3, 11),
        false,
        Crosshair::Gradient,
        "Tints the photo with a gradient, such as a deeper sky.",
        "The start is a point of the stored frame; Angle points from it to the tinted side, degrees clockwise from up (0: up); Feather is the ramp as a fraction of the shorter side.",
    ),
    // Tab 4, More effects (editing.md §8.3).
    fx(
        "infrared",
        "Infrared Film",
        "infrared(fade)",
        &[fade("0")],
        false,
        (4, 0),
        false,
        Crosshair::None,
        "Black and white infrared film: bright foliage, dark skies.",
        "",
    ),
    fx(
        "plastic_lens",
        "Plastic Lens",
        "plastic_lens(blur,grain,fade)",
        &[number("blur", "Blur Edges", looks::BLUR_EDGES, "70"), number("grain", "Grain", looks::GRAIN, "30"), fade("0")],
        false,
        (4, 1),
        false,
        Crosshair::None,
        "The soft edges, grain and dark corners of a toy camera.",
        "",
    ),
    fx(
        "hdr",
        "HDR Look",
        "hdr(radius,strength,fade)",
        &[number("radius", "Radius", looks::HDR_RADIUS, "20"), number("strength", "Strength", looks::HDR_STRENGTH, "3"), fade("0")],
        false,
        (4, 2),
        false,
        Crosshair::None,
        "Brings out detail with strong local contrast.",
        "Radius in thousandths of the shorter side.",
    ),
    fx(
        "widescreen",
        "Widescreen Film",
        "widescreen(letterbox)",
        &[flag("letterbox", "Letterbox", "true")],
        true,
        (4, 3),
        false,
        Crosshair::None,
        "Crops to a wide 1.7:1 frame with a film look and black bars. Changes the picture's size.",
        SIZE_NOTE,
    ),
    fx(
        "dream_glow",
        "Dream Glow",
        "dream_glow(bloom,brightness,fade)",
        &[number("bloom", "Bloom", looks::BLOOM, "25"), number("brightness", "Brightness", looks::BRIGHTNESS, "50"), fade("0")],
        false,
        (4, 4),
        false,
        Crosshair::None,
        "A soft, bright, dreamy glow.",
        "Bloom in thousandths of the shorter side.",
    ),
    fx(
        "invert",
        "Invert Colors",
        "invert()",
        NONE,
        false,
        (4, 5),
        true,
        Crosshair::None,
        "Makes a negative, or turns a scanned negative positive.",
        "One click.",
    ),
    fx(
        "duotone",
        "Duo-Tone",
        "duotone(first,second,brightness,contrast,fade)",
        &[
            colour("first", "First color", "#004488"),
            colour("second", "Second color", "#ffff00"),
            number("brightness", "Brightness", remap::BRIGHTNESS, "0"),
            number("contrast", "Contrast", remap::CONTRAST, "20"),
            fade("0"),
        ],
        false,
        (4, 6),
        false,
        Crosshair::None,
        "Maps the photo's tones onto two colours.",
        "",
    ),
    // Tab 5, Even more effects (editing.md §8.4).
    fx(
        "boost",
        "Boost",
        "boost(strength)",
        &[number("strength", "Strength", remap::STRENGTH, "50")],
        false,
        (5, 0),
        false,
        Crosshair::None,
        "Stronger colour and contrast.",
        "",
    ),
    fx(
        "soften",
        "Soften",
        "soften(softness,fade;mode,r,x,y,…;…)",
        &[
            number("softness", "Softness", soften::SOFTNESS, "50"),
            fade("50"),
            Param {
                name: "strokes",
                label: "Brush strokes",
                kind: Kind::Strokes { max: soften::MAX_STROKES, fields: &BRUSH },
                default: "",
                log: false,
            },
        ],
        false,
        (5, 1),
        false,
        Crosshair::None,
        "Softens the photo, or only where you paint.",
        "Without strokes it applies to the whole photo; each stroke paints or erases where it applies, its points in the stored frame, its radius relative to the shorter side.",
    ),
    fx(
        "vignette",
        "Vignette",
        "vignette(size,strength,color,fade)",
        &[
            number("size", "Size", stylize::VIGNETTE_SIZE, "35"),
            number("strength", "Strength", stylize::VIGNETTE_STRENGTH, "1.4"),
            colour("color", "Color", "#000000"),
            fade("0"),
        ],
        false,
        (5, 2),
        false,
        Crosshair::None,
        "Darkens the edges, or takes them toward a colour.",
        "Size in percent of the half-diagonal. Repeatable.",
    ),
    fx(
        "pixelate",
        "Pixelate",
        "pixelate(size,mode,fade)",
        &[
            number("size", "Pixel Size", stylize::PIXEL_SIZE, "20"),
            Param {
                name: "mode",
                label: "Blend mode",
                kind: Kind::Choice(&["normal", "multiply", "screen", "overlay", "soft_light"]),
                default: "normal",
                log: false,
            },
            fade("0"),
        ],
        false,
        (5, 3),
        false,
        Crosshair::None,
        "Turns the photo into large square pixels.",
        "Pixel Size in thousandths of the shorter side.",
    ),
    fx(
        "focal_zoom",
        "Focal Zoom",
        "focal_zoom(x,y,size,amount,hardness,fade)",
        &[
            number("x", "Centre x", UNIT, "0.5"),
            number("y", "Centre y", UNIT, "0.5"),
            number("size", "Focal Size", stylize::FOCAL_SIZE, "0.3"),
            number("amount", "Zoom Amount", stylize::ZOOM_AMOUNT, "50"),
            number("hardness", "Edge Hardness", stylize::HARDNESS, "50"),
            fade("0"),
        ],
        false,
        (5, 4),
        false,
        Crosshair::Circle,
        "A zoom blur outside a sharp circle.",
        "The centre is a point of the stored frame; Focal Size is the circle's radius as a fraction of the shorter side.",
    ),
    fx(
        "border",
        "Border",
        "border(outer_color,outer,inner_color,inner,radius,caption)",
        &[
            colour("outer_color", "Outer color", "#000000"),
            number("outer", "Outer thickness", canvas::THICKNESS, "20"),
            colour("inner_color", "Inner color", "#ffffff"),
            number("inner", "Inner thickness", canvas::THICKNESS, "5"),
            number("radius", "Corner radius", canvas::CORNER, "0"),
            number("caption", "Caption height", canvas::CAPTION, "0"),
        ],
        true,
        (5, 5),
        false,
        Crosshair::None,
        "Frames the photo with two bands of colour. Changes the picture's size.",
        "Thicknesses and the caption height in thousandths of the shorter side; Corner radius in percent (100: an oval). Changes the picture's size: later effects work on the larger picture, a later crop still crops the photo.",
    ),
    fx(
        "drop_shadow",
        "Drop Shadow",
        "drop_shadow(color,distance,angle,blur,fade,background)",
        &[
            colour("color", "Shadow color", "#000000"),
            number("distance", "Distance", mats::DISTANCE, "4"),
            number("angle", "Angle", mats::ANGLE, "90"),
            number("blur", "Blur", mats::BLUR, "10"),
            fade("30"),
            colour("background", "Background", "#ffffff"),
        ],
        true,
        (5, 6),
        false,
        Crosshair::None,
        "Floats the photo over a background with a shadow. Changes the picture's size.",
        "Distance and Blur in thousandths of the shorter side; Angle is where the shadow falls, degrees clockwise from up (90: to the right). Changes the picture's size: later effects work on the larger picture, a later crop still crops the photo.",
    ),
    fx(
        "museum_matte",
        "Museum Matte",
        "museum_matte(outer_color,outer,inner_color,inner)",
        &[
            colour("outer_color", "Outer color", "#1a0e03"),
            number("outer", "Outer thickness", mats::THICKNESS, "25"),
            colour("inner_color", "Inner color", "#f0eae4"),
            number("inner", "Inner thickness", mats::THICKNESS, "40"),
        ],
        true,
        (5, 7),
        false,
        Crosshair::None,
        "Mounts the photo in a double mat. Changes the picture's size.",
        "Thicknesses in thousandths of the shorter side. Repeatable. Changes the picture's size: later effects work on the larger picture, a later crop still crops the photo.",
    ),
];
