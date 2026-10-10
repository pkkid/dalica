//! The keymap (`menus-and-shortcuts.md` §2.0), pure: key parsing, default bindings from the
//! command specs, user overrides, resolution and conflicts.
//!
//! - A binding is (command, context, key). `Ctrl` is the primary modifier (Cmd on macOS).
//! - A user override for (command, context) replaces that command's default keys in that context;
//!   an override with no key removes them. Overrides are what `settings.json` stores.
//! - A key press resolves Text input → the active view context → Global; the first binding wins.
//! - Two bindings conflict when they share a key in contexts that can be active together: the same
//!   context, or Global and any other. The same key in two view contexts is fine.

use std::fmt;

use crate::registry::Context;

/// A key with its modifiers, in one canonical form.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// `A`–`Z`, `0`–`9`, `F1`–`F24`, a named key (`Enter`, `Esc`, `Up`…) or one punctuation character.
    pub code: String,
}

const NAMED: [&str; 16] =
    ["Enter", "Esc", "Tab", "Space", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown", "Up", "Down", "Left", "Right", "Plus"];
const PUNCTUATION: &str = ",./\\;'[]-=`";

impl Key {
    /// Parses `Ctrl+Shift+Z`, `F4`, `Alt+Enter`, `Ctrl+,` … Modifiers in any order and case
    /// (`Cmd` and `Ctrl` are the same; `Option` is `Alt`); the key name in any case.
    pub fn parse(text: &str) -> Result<Key, String> {
        let bad = || format!("`{text}` is not a key");
        if text.len() > 64 {
            return Err(bad());
        }
        let mut parts: Vec<&str> = text.split('+').collect();
        // `Ctrl++` / `Ctrl+Plus`: a trailing empty part means the key itself is `+`.
        if text.ends_with("++") {
            parts.truncate(parts.len().saturating_sub(2));
            parts.push("Plus");
        }
        let (code, mods) = parts.split_last().ok_or_else(bad)?;
        let mut key = Key { ctrl: false, alt: false, shift: false, code: String::new() };
        for m in mods {
            let flag = match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "control" | "command" => &mut key.ctrl,
                "alt" | "option" | "opt" => &mut key.alt,
                "shift" => &mut key.shift,
                _ => return Err(bad()),
            };
            if *flag {
                return Err(bad());
            }
            *flag = true;
        }
        key.code = canonical_code(code.trim()).ok_or_else(bad)?;
        Ok(key)
    }
}

fn canonical_code(code: &str) -> Option<String> {
    let mut chars = code.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => return Some(c.to_ascii_uppercase().to_string()),
        (Some(c), None) if PUNCTUATION.contains(c) => return Some(c.to_string()),
        _ => {}
    }
    let lower = code.to_ascii_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=24).contains(n)) {
        return Some(format!("F{n}"));
    }
    let alias = match lower.as_str() {
        "escape" => "esc",
        "return" => "enter",
        "del" => "delete",
        "pgup" => "pageup",
        "pgdn" | "pgdown" => "pagedown",
        other => other,
    };
    NAMED.iter().find(|n| n.eq_ignore_ascii_case(alias)).map(|n| (*n).to_string())
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                f.write_str(name)?;
            }
        }
        f.write_str(&self.code)
    }
}

/// One binding in effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub command: String,
    pub context: Context,
    pub key: Key,
}

/// A user override for (command, context): the keys that replace the defaults (none = removed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Override {
    pub command: String,
    pub context: Context,
    pub keys: Vec<Key>,
}

/// Whether bindings in contexts `a` and `b` can be active at the same time.
pub fn overlapping(a: Context, b: Context) -> bool {
    a == b || a == Context::Global || b == Context::Global
}

/// The defaults and the user's overrides.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    defaults: Vec<Binding>,
    overrides: Vec<Override>,
}

impl Keymap {
    /// A keymap from default bindings `(command, context, key text)`. Every key must parse.
    pub fn new<'a>(defaults: impl IntoIterator<Item = (&'a str, Context, &'a str)>) -> Result<Keymap, String> {
        let defaults = defaults
            .into_iter()
            .map(|(c, ctx, k)| Ok(Binding { command: c.to_string(), context: ctx, key: Key::parse(k)? }))
            .collect::<Result<_, String>>()?;
        Ok(Keymap { defaults, overrides: Vec::new() })
    }

    /// The defaults declared by every command spec.
    pub fn from_specs(specs: &[crate::registry::CommandSpec]) -> Result<Keymap, String> {
        Keymap::new(specs.iter().flat_map(|s| s.keys.iter().map(move |(ctx, k)| (s.id, *ctx, *k))))
    }

    pub fn overrides(&self) -> &[Override] {
        &self.overrides
    }

    pub fn set_overrides(&mut self, overrides: Vec<Override>) {
        self.overrides = overrides;
    }

    /// Every binding in effect.
    pub fn bindings(&self) -> Vec<Binding> {
        let overridden = |b: &Binding| self.overrides.iter().any(|o| o.command == b.command && o.context == b.context);
        let mut out: Vec<Binding> = self.defaults.iter().filter(|b| !overridden(b)).cloned().collect();
        for o in &self.overrides {
            out.extend(o.keys.iter().map(|k| Binding { command: o.command.clone(), context: o.context, key: k.clone() }));
        }
        out
    }

    /// The keys of `command` in `context`.
    pub fn keys_for(&self, command: &str, context: Context) -> Vec<Key> {
        self.bindings().into_iter().filter(|b| b.command == command && b.context == context).map(|b| b.key).collect()
    }

    /// The command a key press runs: Text input (while a text field has focus), then `view`, then Global.
    pub fn resolve(&self, key: &Key, view: Context, text_input: bool) -> Option<String> {
        let bindings = self.bindings();
        let order = [text_input.then_some(Context::TextInput), Some(view), Some(Context::Global)];
        order.into_iter().flatten().find_map(|ctx| bindings.iter().find(|b| b.context == ctx && &b.key == key).map(|b| b.command.clone()))
    }

    /// Pairs of bindings that clash.
    pub fn conflicts(&self) -> Vec<(Binding, Binding)> {
        let b = self.bindings();
        let mut out = Vec::new();
        for (i, x) in b.iter().enumerate() {
            for y in b.iter().skip(i + 1) {
                if x.key == y.key && overlapping(x.context, y.context) && x.command != y.command {
                    out.push((x.clone(), y.clone()));
                }
            }
        }
        out
    }

    /// Binds `key` (or, with `None`, no key) to `command` in `context`, replacing its keys there.
    /// A clash with another command is refused with the editor's message, unless `replace`, which
    /// removes the key from the other command.
    pub fn set(&mut self, command: &str, context: Context, key: Option<Key>, replace: bool) -> Result<(), String> {
        if let Some(k) = &key {
            let clashes: Vec<Binding> =
                self.bindings().into_iter().filter(|b| &b.key == k && overlapping(b.context, context) && b.command != command).collect();
            if let Some(c) = clashes.first()
                && !replace
            {
                return Err(format!("{k} is already {} in {:?}.", c.command, c.context));
            }
            for c in clashes {
                let keys = self.keys_for(&c.command, c.context).into_iter().filter(|x| x != k).collect();
                self.put(Override { command: c.command, context: c.context, keys });
            }
        }
        self.put(Override { command: command.to_string(), context, keys: key.into_iter().collect() });
        Ok(())
    }

    fn put(&mut self, o: Override) {
        self.overrides.retain(|x| !(x.command == o.command && x.context == o.context));
        self.overrides.push(o);
    }

    /// Drops the user's overrides for `command` (all contexts), or all of them.
    pub fn reset(&mut self, command: Option<&str>) {
        match command {
            Some(c) => self.overrides.retain(|o| o.command != c),
            None => self.overrides.clear(),
        }
    }
}
