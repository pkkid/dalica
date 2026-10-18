//! The edit-operation catalogue (`catalog`, M10.9; the effects, M12.1): every operation the parser
//! knows is listed once, in the parser's order, with the label its module gives and a syntax naming
//! its parameters; each range is what the module clamps to and each choice, colour and checkbox
//! what it reads; every default prints back as Dalica writes it and renders the same pixels every
//! time, and an operation this build does not render is skipped. The 27 effects stand in their
//! tabs' grids in the spec's order, at most 12 to a tab.

use crate::common::{run, scene};
use dalica_edit::catalog::{Crosshair, Kind, MAX_TILES, OpInfo, Param, TABS, catalog, effects, lookup};
use dalica_edit::{LISTED, Op, Typed, ops};

/// `v` as an entry writes it.
fn text(v: f64) -> String {
    v.to_string()
}

/// The parameters an entry writes before its strokes, and its strokes' fields (none without).
fn parts(op: &OpInfo) -> (Vec<&'static Param>, &'static [Param]) {
    let mut plain = Vec::new();
    let mut stroke: &'static [Param] = &[];
    for p in op.params {
        match p.kind {
            Kind::Strokes { fields, .. } => stroke = fields,
            _ => plain.push(p),
        }
    }
    (plain, stroke)
}

/// Every field of one entry in the order the text writes them: the parameters, then one stroke.
fn fields(op: &OpInfo) -> Vec<&'static Param> {
    let (mut plain, stroke) = parts(op);
    plain.extend(stroke);
    plain
}

/// Where a test starts a field: its default, or a value inside its range.
fn base(p: &Param) -> String {
    match p.kind {
        _ if !p.default.is_empty() => p.default.to_string(),
        Kind::Number { min, max } => text((min + max) / 2.0),
        Kind::Choice(choices) => choices.first().map_or(String::new(), |c| c.to_string()),
        Kind::Colour { .. } => "#808080".into(),
        Kind::Flag => "false".into(),
        Kind::Open | Kind::Seconds | Kind::Strokes { .. } => String::new(),
    }
}

/// The parameter text of `op`: `head` (the parameters before the strokes) and `strokes` strokes
/// of `stroke`, as an entry writes them.
fn join(head: &[String], stroke: &[String], strokes: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    if !head.is_empty() {
        out.push(head.join(","));
    }
    out.extend(std::iter::repeat_n(stroke.join(","), strokes));
    out.join(";")
}

/// The parameter text of `op` with field `i` (of [`fields`]) set to `value`, the others at their
/// base, with one stroke when the operation takes strokes.
fn with(op: &OpInfo, i: usize, value: &str) -> String {
    let (plain, stroke) = parts(op);
    let v: Vec<String> = fields(op).iter().enumerate().map(|(j, p)| if j == i { value.to_string() } else { base(p) }).collect();
    let (head, tail) = v.split_at(plain.len());
    join(head, tail, usize::from(!stroke.is_empty()))
}

/// What `op`'s module prints for `params`, split into its fields; `None` when it refuses them.
fn read(op: &OpInfo, params: &str) -> Option<Vec<String>> {
    let typed = Typed::parse(op.name, params).expect("a typed operation").ok()?;
    Some(typed.params().split([',', ';']).map(str::to_string).collect())
}

/// The effects of each tab in grid order, as `editing.md` §8.2–§8.4 lists them.
const GRIDS: [(u8, &[&str]); 3] = [
    (3, &["sharpen", "sepia", "bw", "warmify", "grain", "tint", "saturation", "soft_focus", "glow", "filtered_bw", "focal_bw", "graduated_tint"]),
    (4, &["infrared", "plastic_lens", "hdr", "widescreen", "dream_glow", "invert", "duotone"]),
    (5, &["boost", "soften", "vignette", "pixelate", "focal_zoom", "border", "drop_shadow", "museum_matte"]),
];

#[test]
fn every_operation_the_parser_knows_is_listed_once_in_its_order() {
    let names: Vec<&str> = ["rotate", "flip_h", "flip_v"].into_iter().chain(LISTED).collect();
    let listed: Vec<&str> = catalog().iter().map(|op| op.name).collect();
    assert_eq!(listed, names, "a parser name missing from the catalogue, or one it does not know");
    for op in catalog() {
        assert_eq!(lookup(op.name), Some(op));
        assert_eq!(Some(op.label), ops::label(op.name), "{}: the module's label", op.name);
        // A name the parser knows, whatever its parameters.
        let probe = Op::parse(&op.default_entry().unwrap_or_else(|| format!("{}(1)", op.name)));
        assert_eq!(probe.name(), Some(op.name), "{}", op.name);
        // The syntax names the parameters in order (a stroke's fields after the others).
        let names: Vec<&str> = fields(op).iter().map(|p| p.name).collect();
        let inner = op.syntax.strip_prefix(op.name).and_then(|s| s.strip_prefix('(')).and_then(|s| s.strip_suffix(')'));
        let inner = inner.unwrap_or_else(|| panic!("{}: syntax {:?}", op.name, op.syntax));
        let written: Vec<&str> = inner.trim_end_matches(";…").split([',', ';']).filter(|s| !s.is_empty() && *s != "…").collect();
        assert_eq!(written, names, "{}", op.name);
        // What renders is what M6 and M7 brought, and each effect (M12) once its module renders
        // (`VERSION` 1 or more; 0 is a stub); trimming (M15) is kept and skipped. Every operation
        // but trimming has a module that reads its parameters, with every default and range
        // settled.
        let live = ops::version(op.name).is_some_and(|v| v > 0);
        let renders = match op.milestone {
            "M6" | "M7" => true,
            "M12" => live,
            _ => false,
        };
        assert_eq!(op.renders(), renders, "{}", op.name);
        assert_eq!(live, renders, "{}: renders exactly when its module's version is 1 or more", op.name);
        assert_eq!(op.typed(), op.milestone != "M15", "{}", op.name);
        if op.typed() {
            assert!(op.default_entry().is_some(), "{}: a typed operation has every default settled", op.name);
            assert!(fields(op).iter().all(|p| p.kind != Kind::Open), "{}: and every range", op.name);
        }
    }
    let sized: Vec<&str> = catalog().iter().filter(|op| op.changes_size).map(|op| op.name).collect();
    assert_eq!(sized, ["rotate", "crop", "widescreen", "border", "drop_shadow", "museum_matte"]);
}

#[test]
fn the_27_effects_stand_in_their_tabs_grids_in_order() {
    let mut total = 0;
    for (tab, names) in GRIDS {
        let tiles: Vec<_> = effects(tab).collect();
        assert_eq!(tiles.iter().map(|(op, _)| op.name).collect::<Vec<_>>(), names, "tab {tab}");
        assert!(tiles.len() <= MAX_TILES, "tab {tab}: {} tiles", tiles.len());
        for (place, (op, e)) in tiles.iter().enumerate() {
            assert_eq!(usize::from(e.place), place, "{}: its place in the grid", op.name);
            assert_eq!(op.milestone, "M12", "{}", op.name);
            assert!(!e.tooltip.is_empty() && e.tooltip.ends_with('.'), "{}: {:?}", op.name, e.tooltip);
            // The effects that change the picture's size say so in their tooltip, and only they.
            assert_eq!(e.tooltip.contains("Changes the picture's size."), op.changes_size, "{}", op.name);
            // A one-click effect has nothing to set; a crosshair effect has its point and the
            // crosshair's own parameter.
            assert_eq!(e.one_click, op.params.is_empty(), "{}", op.name);
            let named = |n: &str| op.params.iter().any(|p| p.name == n);
            match e.crosshair {
                Crosshair::None => assert!(!named("x") && !named("y"), "{}", op.name),
                Crosshair::Circle => assert!(named("x") && named("y") && named("size"), "{}", op.name),
                Crosshair::Gradient => assert!(named("x") && named("y") && named("angle"), "{}", op.name),
            }
            // At most four sliders on a panel (`editing.md` §8.1), the crosshair's point aside.
            let sliders = op.params.iter().filter(|p| matches!(p.kind, Kind::Number { .. }) && !["x", "y"].contains(&p.name)).count();
            assert!(sliders <= 4, "{}: {sliders} sliders", op.name);
        }
        total += tiles.len();
    }
    assert_eq!(total, 27);
    assert_eq!(catalog().iter().filter(|op| op.effect.is_some()).count(), 27, "every effect is on a tab");
    assert!(effects(2).next().is_none() && effects(6).next().is_none());
    assert_eq!(TABS, [3, 4, 5]);
    let one_click: Vec<&str> = catalog().iter().filter(|op| op.effect.is_some_and(|e| e.one_click)).map(|op| op.name).collect();
    assert_eq!(one_click, ["sepia", "bw", "warmify", "invert"]);
    let crosshairs: Vec<&str> = catalog().iter().filter(|op| op.effect.is_some_and(|e| e.crosshair != Crosshair::None)).map(|op| op.name).collect();
    assert_eq!(crosshairs, ["soft_focus", "focal_bw", "graduated_tint", "focal_zoom"]);
    let logs: Vec<&str> = catalog().iter().flat_map(|op| op.params.iter().filter(|p| p.log).map(|_| op.name)).collect();
    assert_eq!(logs, ["glow"], "only Glow's Radius is on a logarithmic slider");
}

#[test]
fn ranges_choices_colours_and_checkboxes_are_what_the_modules_read() {
    for op in catalog().iter().filter(|op| op.typed()) {
        for (i, p) in fields(op).iter().enumerate() {
            let at = |value: &str| read(op, &with(op, i, value)).map(|f| f[i].clone());
            match p.kind {
                Kind::Number { min, max } => {
                    assert!(min < max, "{}.{}", op.name, p.name);
                    // Inside the range a value reads back as written.
                    for inside in [min + (max - min) / 4.0, max - (max - min) / 4.0] {
                        assert_eq!(at(&text(inside)), Some(text(inside)), "{}.{} at {inside}", op.name, p.name);
                    }
                    // Beyond a bound the module reads the bound itself, as written. (A crop whose
                    // rectangle is then empty is refused either way, so one side at least is read.)
                    let mut read_a_side = false;
                    for (bound, beyond) in [(min, min - 1.0), (max, max + 1.0)] {
                        let (at_bound, past) = (at(&text(bound)), at(&text(beyond)));
                        assert!(at_bound.as_ref().is_none_or(|v| *v == text(bound)), "{}.{} at {bound}: {at_bound:?}", op.name, p.name);
                        assert_eq!(past, at_bound, "{}.{}: {beyond} clamps to {bound}", op.name, p.name);
                        read_a_side |= past.is_some();
                    }
                    assert!(read_a_side, "{}.{}: read past one bound", op.name, p.name);
                }
                Kind::Choice(choices) => {
                    for c in choices {
                        assert_eq!(at(c).as_deref(), Some(*c), "{}.{}", op.name, p.name);
                    }
                    assert_eq!(at("45"), None, "{}.{}", op.name, p.name);
                }
                Kind::Colour { none } => {
                    assert_eq!(at("#80cfff").as_deref(), Some("#80cfff"), "{}.{}", op.name, p.name);
                    assert_eq!(at("none").is_some(), none, "{}.{}", op.name, p.name);
                    assert_eq!(at("#80cfzz"), None, "{}.{}", op.name, p.name);
                }
                Kind::Flag => {
                    assert_eq!(at("true").as_deref(), Some("true"), "{}.{}", op.name, p.name);
                    assert_eq!(at("false").as_deref(), Some("false"), "{}.{}", op.name, p.name);
                    assert_eq!(at("1"), None, "{}.{}", op.name, p.name);
                }
                Kind::Open | Kind::Seconds | Kind::Strokes { .. } => panic!("{}.{}: no module reads {:?}", op.name, p.name, p.kind),
            }
        }
        if let Some(Param { kind: Kind::Strokes { max, .. }, .. }) = op.params.last() {
            let (plain, stroke) = parts(op);
            let head: Vec<String> = plain.iter().map(|p| base(p)).collect();
            let one: Vec<String> = stroke.iter().map(base).collect();
            assert!(Typed::parse(op.name, &join(&head, &one, *max)).is_some_and(|r| r.is_ok()), "{}: {max} strokes", op.name);
            assert!(Typed::parse(op.name, &join(&head, &one, max + 1)).is_some_and(|r| r.is_err()), "{}: {max} + 1", op.name);
        }
    }
}

#[test]
fn defaults_print_back_and_render_the_same_every_time() {
    let img = scene(11, 48, 32);
    let untouched = run(&img, &[]);
    let mut rendered = 0;
    for op in catalog() {
        let Some(entry) = op.default_entry() else { continue };
        let parsed = Op::parse(&entry);
        assert_eq!(parsed.to_string(), entry);
        if op.typed() {
            let inner = entry.strip_prefix(op.name).and_then(|s| s.strip_prefix('(')).and_then(|s| s.strip_suffix(')')).unwrap();
            let typed = Typed::parse(op.name, inner).expect("a module").unwrap_or_else(|e| panic!("{entry}: {e}"));
            assert_eq!(Op::from(typed).to_string(), entry, "the default as Dalica writes it");
        }
        let first = run(&img, &[&entry]);
        assert_eq!(run(&img, &[&entry]), first, "{entry}");
        if op.renders() {
            rendered += 1;
        } else {
            assert_eq!(first, untouched, "{entry}: skipped until its module renders it");
        }
    }
    assert_eq!(rendered, catalog().iter().filter(|op| op.renders()).count(), "every rendered operation has a default entry");
}

/// Until its renderer lands (M12.6–M12.14) an effect is a stub (`VERSION` 0): a recipe holding
/// every stub, twice, renders exactly as the same recipe without them; every effect that renders
/// changes the picture at its defaults, among the same operations.
#[test]
fn a_recipe_with_every_stub_effect_renders_as_the_recipe_without_them() {
    let img = scene(5, 40, 30);
    let around = ["crop(0.1,0.1,0.9,0.9)", "autofix()", "tuning(0.2,0.1,0.05,-0.3,none)", "rotate(90)"];
    let effects: Vec<&OpInfo> = catalog().iter().filter(|op| op.effect.is_some()).collect();
    assert_eq!(effects.len(), 27);
    let entry = |op: &OpInfo| op.default_entry().unwrap_or_else(|| panic!("{}: a default entry", op.name));
    let (live, stubs): (Vec<&OpInfo>, Vec<&OpInfo>) = effects.into_iter().partition(|op| op.renders());
    let stubs: Vec<String> = stubs.into_iter().map(entry).collect();
    let mut with: Vec<&str> = vec![around[0], around[1]];
    with.extend(stubs.iter().map(String::as_str));
    with.extend(&around[2..]);
    with.extend(stubs.iter().map(String::as_str));
    let plain = run(&img, &around);
    assert_eq!(run(&img, &with), plain);
    for op in live {
        let e = entry(op);
        assert_ne!(run(&img, &[around[0], around[1], &e, around[2], around[3]]), plain, "{e} renders");
    }
}
