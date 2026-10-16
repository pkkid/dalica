//! The edit-operation catalogue (`catalog`, M10.9): every operation the parser knows is listed once,
//! in the parser's order, with the label its module gives and a syntax naming its parameters; each
//! range is what the module clamps to and each choice and colour what it reads; every settled
//! default prints back as Dalica writes it and renders the same pixels every time, and an operation
//! this build does not render is skipped.

use crate::common::{run, scene};
use dalica_edit::catalog::{Kind, OpInfo, Param, catalog, lookup};
use dalica_edit::{LISTED, Op, Typed, ops};

/// `v` as an entry writes it.
fn text(v: f64) -> String {
    v.to_string()
}

/// Where a test starts a number: its default, or the middle of its range.
fn base(p: &Param) -> String {
    match p.kind {
        Kind::Number { min, max } if p.default.is_empty() => text((min + max) / 2.0),
        _ => p.default.to_string(),
    }
}

/// The parameter text with field `i` of `fields` set to `value`, the others at their base.
fn with(fields: &[Param], i: usize, value: &str) -> String {
    fields.iter().enumerate().map(|(j, p)| if j == i { value.to_string() } else { base(p) }).collect::<Vec<_>>().join(",")
}

/// What `op`'s module prints for `params`, split into its fields; `None` when it refuses them.
fn read(op: &OpInfo, params: &str) -> Option<Vec<String>> {
    let typed = Typed::parse(op.name, params).expect("a typed operation").ok()?;
    Some(typed.params().split([',', ';']).map(str::to_string).collect())
}

/// The fields of one entry of `op` and how they sit in its parameter text: the parameters, or one
/// stroke's fields.
fn fields(op: &OpInfo) -> &'static [Param] {
    match op.params {
        [Param { kind: Kind::Strokes { fields, .. }, .. }] => fields,
        other => other,
    }
}

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
        // The syntax names the parameters in order.
        let names: Vec<&str> = fields(op).iter().map(|p| p.name).collect();
        let inner = op.syntax.strip_prefix(op.name).and_then(|s| s.strip_prefix('(')).and_then(|s| s.strip_suffix(')'));
        let inner = inner.unwrap_or_else(|| panic!("{}: syntax {:?}", op.name, op.syntax));
        assert_eq!(inner.trim_end_matches(";…").split(',').filter(|s| !s.is_empty()).collect::<Vec<_>>(), names, "{}", op.name);
        // What renders is what M6 and M7 brought; the effects (M12) and trimming (M15) are kept
        // and skipped, and only operations with a module read their parameters.
        assert_eq!(op.renders(), ["M6", "M7"].contains(&op.milestone), "{}", op.name);
        assert_eq!(op.typed(), op.renders(), "{}", op.name);
        if op.typed() {
            assert!(op.default_entry().is_some(), "{}: a rendered operation has every default settled", op.name);
            assert!(fields(op).iter().all(|p| p.kind != Kind::Open), "{}: and every range", op.name);
        }
    }
    let sized: Vec<&str> = catalog().iter().filter(|op| op.changes_size).map(|op| op.name).collect();
    assert_eq!(sized, ["rotate", "crop"]);
}

#[test]
fn ranges_choices_and_colours_are_what_the_modules_read() {
    for op in catalog().iter().filter(|op| op.typed()) {
        let fields = fields(op);
        for (i, p) in fields.iter().enumerate() {
            let at = |value: &str| read(op, &with(fields, i, value)).map(|f| f[i].clone());
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
                Kind::Open | Kind::Flag | Kind::Seconds | Kind::Strokes { .. } => panic!("{}.{}: no module reads {:?} yet", op.name, p.name, p.kind),
            }
        }
        if let [Param { kind: Kind::Strokes { max, .. }, .. }] = op.params {
            let stroke = with(fields, usize::MAX, "");
            assert!(Typed::parse(op.name, &vec![stroke.as_str(); *max].join(";")).is_some_and(|r| r.is_ok()), "{}: {max} strokes", op.name);
            assert!(Typed::parse(op.name, &vec![stroke.as_str(); max + 1].join(";")).is_some_and(|r| r.is_err()), "{}: {max} + 1", op.name);
        }
    }
}

#[test]
fn settled_defaults_print_back_and_render_the_same_every_time() {
    let img = scene(11, 48, 32);
    let untouched = run(&img, &[]);
    let mut rendered = 0;
    for op in catalog() {
        let Some(entry) = op.default_entry() else { continue };
        let parsed = Op::parse(&entry);
        assert_eq!(parsed.to_string(), entry);
        if op.typed() {
            let typed = parsed.typed().expect("a module").unwrap_or_else(|e| panic!("{entry}: {e}"));
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
