//! The edit-operation catalogue (`mcp.md` §4.2 `list_edit_ops`, `data-model.md` §3.1, `editing.md`
//! §6–§8, §11): one row per operation the parser knows, with its English label, its parameters
//! (type, range, default), whether it may change the picture's size, the milestone that brings
//! it, and whether this build renders it. Agents read it to build recipes; the tests check it
//! against the parser and against each module's clamps, so the rows cannot drift from what Dalica
//! does.
//!
//! - The rows follow the parser: `rotate`, `flip_h`, `flip_v` and every name of [`crate::LISTED`],
//!   in that order. An operation the parser does not know yet (the 15 effects of tabs 4–5) joins
//!   with its syntax (M12.1), never before: a row is an operation a recipe can hold.
//! - Ranges and defaults of operations with a module are the module's own constants, so a row
//!   says what the module clamps to. The effects' rows give the controls `editing.md` §8 describes;
//!   where it leaves a range or default open, the row says so ([`Kind::Open`], an empty default)
//!   and M12.1 settles it with the effect's module.
//! - [`OpInfo::renders`] asks the operation's module, so a row turns on with its renderer.

use crate::ops::{self, retouch, straighten, tone};

/// What one parameter holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A number brought into `min..=max` when read (a module clamps rather than refuses).
    Number { min: f64, max: f64 },
    /// A number whose range is decided with its operation's module (the effects, M12.1).
    Open,
    /// One of these words, exactly.
    Choice(&'static [&'static str]),
    /// An sRGB colour `#rrggbb`; with `none`, also the word `none` (no colour).
    Colour { none: bool },
    /// A checkbox.
    Flag,
    /// A time in seconds from the start of a video, 0 to its length.
    Seconds,
    /// A list of strokes separated by `;`, at most `max`, each holding `fields` separated by `,`.
    /// No strokes (`retouch()`) is the default.
    Strokes { max: usize, fields: &'static [Param] },
}

/// One parameter of an operation, in the order the entry writes them, separated by `,`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Param {
    /// The parameter's name in the syntax (`x0`, `fill`, `deg`).
    pub name: &'static str,
    /// The English control label (`editing.md`), or a short description where no control shows it.
    pub label: &'static str,
    pub kind: Kind,
    /// The default as the entry writes it; empty while the spec leaves it open (M12.1).
    pub default: &'static str,
}

/// The milestone that brought (or brings) an operation.
pub type Milestone = &'static str;

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
    /// Whether the operation may change the picture's width or height (a crop; a quarter turn).
    pub changes_size: bool,
    pub milestone: Milestone,
    /// What else an agent should know (the rules of `data-model.md` §3.1).
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

    /// The entry at its defaults (`crop(0,0,1,1)`, `autofix()`), when every default is settled.
    pub fn default_entry(&self) -> Option<String> {
        let mut fields = Vec::with_capacity(self.params.len());
        for p in self.params {
            if p.default.is_empty() && !matches!(p.kind, Kind::Strokes { .. }) {
                return None;
            }
            fields.push(p.default);
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

const fn number(name: &'static str, label: &'static str, range: (f64, f64), default: &'static str) -> Param {
    Param { name, label, kind: Kind::Number { min: range.0, max: range.1 }, default }
}

const fn open(name: &'static str, label: &'static str, default: &'static str) -> Param {
    Param { name, label, kind: Kind::Open, default }
}

const fn colour(name: &'static str, label: &'static str, default: &'static str) -> Param {
    Param { name, label, kind: Kind::Colour { none: false }, default }
}

const UNIT: (f64, f64) = (0.0, 1.0);
const FADE: (f64, f64) = (0.0, 100.0);
/// How far outside the picture a retouch point may lie (the module's clamp).
const STROKE_POINT: (f64, f64) = (-1.0, 2.0);

const STROKE: [Param; 5] = [
    number("sx", "Source x", STROKE_POINT, ""),
    number("sy", "Source y", STROKE_POINT, ""),
    number("tx", "Target x", STROKE_POINT, ""),
    number("ty", "Target y", STROKE_POINT, ""),
    number("r", "Radius (of the shorter side)", (retouch::MIN_RADIUS, retouch::MAX_RADIUS), ""),
];

const NONE: &[Param] = &[];

static CATALOG: [OpInfo; 24] = [
    OpInfo {
        name: "rotate",
        label: "Rotate",
        syntax: "rotate(deg)",
        params: &[Param { name: "deg", label: "Degrees clockwise", kind: Kind::Choice(&["90", "180", "270"]), default: "90" }],
        changes_size: true,
        milestone: "M6",
        note: "A rotation right after a rotation merges into one entry; a whole turn removes it. A quarter turn swaps width and height.",
    },
    OpInfo {
        name: "flip_h",
        label: "Flip Horizontal",
        syntax: "flip_h()",
        params: NONE,
        changes_size: false,
        milestone: "M6",
        note: "Mirrors left to right; the same flip twice in a row cancels.",
    },
    OpInfo {
        name: "flip_v",
        label: "Flip Vertical",
        syntax: "flip_v()",
        params: NONE,
        changes_size: false,
        milestone: "M6",
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
        note: "A normalised rectangle of the straightened photo in the stored frame; a reversed or empty one is refused. Only the last crop counts.",
    },
    OpInfo {
        name: "straighten",
        label: "Straighten",
        syntax: "straighten(deg)",
        params: &[number("deg", "Degrees clockwise", (-straighten::MAX_DEGREES, straighten::MAX_DEGREES), "0")],
        changes_size: false,
        milestone: "M7",
        note: "Turned about the centre and enlarged so no empty corner shows. Only the last straighten counts.",
    },
    OpInfo {
        name: "retouch",
        label: "Retouch",
        syntax: "retouch(sx,sy,tx,ty,r;…)",
        params: &[Param { name: "strokes", label: "Patches", kind: Kind::Strokes { max: retouch::MAX_STROKES, fields: &STROKE }, default: "" }],
        changes_size: false,
        milestone: "M7",
        note: "Each stroke heals the patch at the target from the source, in order; points in the stored frame, the radius relative to the shorter side.",
    },
    OpInfo {
        name: "autofix",
        label: "Auto Fix",
        syntax: "autofix()",
        params: NONE,
        changes_size: false,
        milestone: "M7",
        note: "Auto Color, then Auto Contrast, then a little Fill Light, measured on the picture at its place in the recipe.",
    },
    OpInfo {
        name: "autocontrast",
        label: "Auto Contrast",
        syntax: "autocontrast()",
        params: NONE,
        changes_size: false,
        milestone: "M7",
        note: "Black and white points and midtones, linked across the channels.",
    },
    OpInfo {
        name: "autocolor",
        label: "Auto Color",
        syntax: "autocolor()",
        params: NONE,
        changes_size: false,
        milestone: "M7",
        note: "Removes a colour cast.",
    },
    OpInfo {
        name: "fill_light",
        label: "Fill Light",
        syntax: "fill_light(a)",
        params: &[number("a", "Fill Light", tone::FILL, "0")],
        changes_size: false,
        milestone: "M7",
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
            Param { name: "neutral", label: "Neutral Color", kind: Kind::Colour { none: true }, default: "none" },
        ],
        changes_size: false,
        milestone: "M7",
        note: "The whole Tuning tab as one entry; the defaults change nothing.",
    },
    OpInfo {
        name: "trim",
        label: "Trim",
        syntax: "trim(start,end)",
        params: &[
            Param { name: "start", label: "Start", kind: Kind::Seconds, default: "" },
            Param { name: "end", label: "End", kind: Kind::Seconds, default: "" },
        ],
        changes_size: false,
        milestone: "M15",
        note: "Videos only.",
    },
    OpInfo {
        name: "sharpen",
        label: "Sharpen",
        syntax: "sharpen(a)",
        params: &[number("a", "Amount", (0.0, 3.0), "0.6")],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
    OpInfo { name: "sepia", label: "Sepia", syntax: "sepia()", params: NONE, changes_size: false, milestone: "M12", note: "One click." },
    OpInfo { name: "bw", label: "B&W", syntax: "bw()", params: NONE, changes_size: false, milestone: "M12", note: "One click." },
    OpInfo { name: "warmify", label: "Warmify", syntax: "warmify()", params: NONE, changes_size: false, milestone: "M12", note: "One click." },
    OpInfo {
        name: "grain",
        label: "Film Grain",
        syntax: "grain(grain,lighten)",
        params: &[number("grain", "Grain", (0.0, 50.0), "10"), Param { name: "lighten", label: "Lighten", kind: Kind::Flag, default: "" }],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
    OpInfo {
        name: "tint",
        label: "Tint",
        syntax: "tint(color,fade)",
        params: &[colour("color", "Tint color", "#80cfff"), number("fade", "Fade", FADE, "0")],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
    OpInfo {
        name: "saturation",
        label: "Saturation",
        syntax: "saturation(a)",
        params: &[number("a", "Amount", (-1.0, 1.0), "0.16")],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
    OpInfo {
        name: "soft_focus",
        label: "Soft Focus",
        syntax: "soft_focus(x,y,size,amount)",
        params: &[number("x", "Centre x", UNIT, "0.5"), number("y", "Centre y", UNIT, "0.5"), open("size", "Size", ""), open("amount", "Amount", "")],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
    OpInfo {
        name: "glow",
        label: "Glow",
        syntax: "glow(i,r)",
        params: &[number("i", "Intensity", UNIT, "0.65"), open("r", "Radius", "3")],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
    OpInfo {
        name: "filtered_bw",
        label: "Filtered B&W",
        syntax: "filtered_bw(color)",
        params: &[colour("color", "Filter color", "")],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
    OpInfo {
        name: "focal_bw",
        label: "Focal B&W",
        syntax: "focal_bw(x,y,size,sharpness)",
        params: &[
            number("x", "Centre x", UNIT, "0.5"),
            number("y", "Centre y", UNIT, "0.5"),
            open("size", "Size", ""),
            open("sharpness", "Sharpness", ""),
        ],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
    OpInfo {
        name: "graduated_tint",
        label: "Graduated Tint",
        syntax: "graduated_tint(y,feather,shade,color)",
        params: &[
            number("y", "Gradient start", UNIT, ""),
            number("feather", "Feather", UNIT, "0.25"),
            number("shade", "Shade", UNIT, "0.25"),
            colour("color", "Color", ""),
        ],
        changes_size: false,
        milestone: "M12",
        note: "",
    },
];
