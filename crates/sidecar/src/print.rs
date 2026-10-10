//! The deterministic printer: keys sorted by their bytes at every level (whatever order the map
//! keeps them in), two-space indentation, LF line endings, a final newline. Scalars and keys use
//! `serde_json`'s escaping, so any string, including any file name, round-trips.

use serde_json::{Map, Value};

pub(crate) fn document(root: &Map<String, Value>) -> Vec<u8> {
    let mut out = String::new();
    object(&mut out, root, 0);
    out.push('\n');
    out.into_bytes()
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn value(out: &mut String, v: &Value, level: usize) {
    match v {
        Value::Object(m) => object(out, m, level),
        Value::Array(a) => array(out, a, level),
        scalar => out.push_str(&scalar.to_string()),
    }
}

fn object(out: &mut String, m: &Map<String, Value>, level: usize) {
    if m.is_empty() {
        out.push_str("{}");
        return;
    }
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    out.push_str("{\n");
    for (i, k) in keys.iter().enumerate() {
        indent(out, level + 1);
        out.push_str(&Value::String((*k).clone()).to_string());
        out.push_str(": ");
        if let Some(v) = m.get(*k) {
            value(out, v, level + 1);
        }
        out.push_str(if i + 1 < keys.len() { ",\n" } else { "\n" });
    }
    indent(out, level);
    out.push('}');
}

fn array(out: &mut String, a: &[Value], level: usize) {
    if a.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push_str("[\n");
    for (i, v) in a.iter().enumerate() {
        indent(out, level + 1);
        value(out, v, level + 1);
        out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
    }
    indent(out, level);
    out.push(']');
}
