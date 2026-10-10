//! `settings.json` in the library directory (`data-model.md` §4, `architecture.md` §4): preferences
//! as a JSON object. Keys this version does not know are kept verbatim; a damaged file is kept aside
//! as `.corrupt-<time>` and defaults are used.
//!
//! M1 reads and writes `"keymap"`: the user's shortcut overrides, as `{command, context, key}`
//! entries (`key: null` = removed). Entries for commands this version does not know, and entries it
//! cannot read, are kept verbatim and ignored (`menus-and-shortcuts.md` §2.0).

use std::collections::BTreeMap;
use std::path::Path;

use dalica_core::{DateTime, Timestamp};
use serde_json::{Map, Value, json};

use crate::keymap::{Key, Override};
use crate::registry::Context;

/// The settings file's name in the library directory.
pub const FILE: &str = "settings.json";
/// The largest settings file read.
const MAX_BYTES: u64 = 64 << 20;

/// The settings document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    root: Map<String, Value>,
}

impl Settings {
    /// Parses a settings file; `Err` (with the reason) unless it is a JSON object.
    pub fn parse(bytes: &[u8]) -> Result<Settings, String> {
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        match serde_json::from_slice::<Value>(bytes) {
            Ok(Value::Object(root)) => Ok(Settings { root }),
            Ok(_) => Err("the top level is not an object".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The file as written: sorted keys, two spaces, a final newline.
    pub fn print(&self) -> Vec<u8> {
        let mut out = serde_json::to_vec_pretty(&Value::Object(self.root.clone())).unwrap_or_default();
        out.push(b'\n');
        out
    }

    /// Reads `<dir>/settings.json`: defaults when absent; a damaged file is set aside (named by
    /// `now`) and the reason returned with the defaults.
    pub fn load(dir: &Path, now: Timestamp) -> (Settings, Option<String>) {
        let path = dir.join(FILE);
        let damaged = |reason: String| {
            let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
            let kept = dalica_fs::set_aside(&path, &format!("corrupt-{stamp}Z")).map(|p| p.display().to_string()).unwrap_or_default();
            (Settings::default(), Some(format!("settings.json: {reason}; kept as {kept}, defaults used")))
        };
        match dalica_fs::read(&path, MAX_BYTES) {
            Ok(None) => (Settings::default(), None),
            Ok(Some(r)) => match Settings::parse(&r.bytes) {
                Ok(s) => (s, None),
                Err(reason) => damaged(reason),
            },
            Err(dalica_fs::Error::TooLarge { .. }) => damaged("too large".into()),
            Err(e) => (Settings::default(), Some(format!("settings.json: {e}"))),
        }
    }

    /// The user's shortcut overrides for commands `known` accepts.
    pub fn keymap(&self, known: &dyn Fn(&str) -> bool) -> Vec<Override> {
        let mut grouped: BTreeMap<(String, Context), Vec<Key>> = BTreeMap::new();
        for entry in self.root.get("keymap").and_then(Value::as_array).into_iter().flatten() {
            if let Some((command, context, key)) = read_entry(entry).filter(|(c, ..)| known(c)) {
                let keys = grouped.entry((command, context)).or_default();
                keys.extend(key);
            }
        }
        grouped.into_iter().map(|((command, context), keys)| Override { command, context, keys }).collect()
    }

    /// Stores `overrides` under `"keymap"`, keeping entries for commands `known` rejects and
    /// entries that cannot be read.
    pub fn set_keymap(&mut self, overrides: &[Override], known: &dyn Fn(&str) -> bool) {
        let mut entries: Vec<Value> = self
            .root
            .get("keymap")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|e| read_entry(e).is_none_or(|(c, ..)| !known(&c)))
            .cloned()
            .collect();
        for o in overrides {
            let context = serde_json::to_value(o.context).unwrap_or(Value::Null);
            if o.keys.is_empty() {
                entries.push(json!({ "command": o.command, "context": context, "key": null }));
            }
            for k in &o.keys {
                entries.push(json!({ "command": o.command, "context": context, "key": k.to_string() }));
            }
        }
        self.root.insert("keymap".into(), Value::Array(entries));
    }
}

/// `(command, context, key)` of one entry; `None` if it cannot be read.
fn read_entry(e: &Value) -> Option<(String, Context, Option<Key>)> {
    let command = e.get("command")?.as_str()?.to_string();
    let context: Context = serde_json::from_value(e.get("context")?.clone()).ok()?;
    let key = match e.get("key")? {
        Value::Null => None,
        Value::String(k) => Some(Key::parse(k).ok()?),
        _ => return None,
    };
    Some((command, context, key))
}
