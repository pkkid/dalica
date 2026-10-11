//! The keymap (`menus-and-shortcuts.md` §2.0), pure: key parsing, default bindings from the
//! command specs, user overrides, resolution and conflicts.
//!
//! - A binding is (command, context, key). `Ctrl` is the primary modifier (Cmd on macOS).
//! - A user override for (command, context) replaces that command's default keys in that context;
//!   an override with no key removes them. Overrides are what `settings.json` stores.
//! - A key press resolves Text input → the active view context → Global; the first binding wins.
//! - Two bindings conflict when they share a key in contexts that can be active together: the same
//!   context, or Global and any other. The same key in two view contexts is fine.
//! - A default may be for some platforms only: `mac:Ctrl+Shift+Z`, `win,linux:Ctrl+Y` (Redo). A
//!   keymap is built for one [`Platform`]; unqualified defaults apply on all three.
//! - The keymap knows every command that may be bound (the engine's, and the UI commands the app
//!   adds with [`Keymap::add_commands`]), with or without default keys.

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

/// The platform a keymap is built for: defaults may differ by platform (`menus-and-shortcuts.md`
/// §2.0: Redo is Ctrl+Y, on macOS Cmd+Shift+Z).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Mac,
    Windows,
    Linux,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Mac, Platform::Windows, Platform::Linux];

    /// The platform this build runs on (other Unixes count as Linux).
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::Mac
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    /// The name a default's qualifier uses: `mac`, `win`, `linux`.
    pub fn name(self) -> &'static str {
        match self {
            Platform::Mac => "mac",
            Platform::Windows => "win",
            Platform::Linux => "linux",
        }
    }
}

/// Parses a default key with its optional platform qualifier (`mac:Ctrl+Shift+Z`,
/// `win,linux:Ctrl+Y`, or plain `Ctrl+8` for all platforms): the key, and whether it applies on
/// `platform`. An unknown platform name is an error.
pub fn parse_default(text: &str, platform: Platform) -> Result<(Key, bool), String> {
    // `:` is never part of a key, so the first one ends the qualifier.
    match text.split_once(':') {
        None => Ok((Key::parse(text)?, true)),
        Some((names, key)) => {
            let mut applies = false;
            for name in names.split(',') {
                let p = Platform::ALL.into_iter().find(|p| p.name() == name.trim()).ok_or_else(|| format!("`{text}`: no platform `{name}`"))?;
                applies |= p == platform;
            }
            Ok((Key::parse(key)?, applies))
        }
    }
}

/// The text of the first default in `keys` that applies on `platform`, without its qualifier.
pub fn first_default(keys: DefaultKeys, platform: Platform) -> Option<&'static str> {
    keys.iter().find_map(|(_, text)| match text.split_once(':') {
        None => Some(*text),
        Some((_, key)) => parse_default(text, platform).ok().filter(|(_, applies)| *applies).map(|_| key),
    })
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

/// A command's default bindings as declared (`CommandSpec::keys`, `UiCommand::keys`).
pub type DefaultKeys = &'static [(Context, &'static str)];

/// The defaults and the user's overrides.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    /// Every command that may be bound, with or without default keys.
    commands: Vec<String>,
    defaults: Vec<Binding>,
    overrides: Vec<Override>,
}

/// The bindings of `defaults` that apply on `platform`. Every key must parse.
fn bindings_on<'a>(defaults: impl IntoIterator<Item = (&'a str, Context, &'a str)>, platform: Platform) -> Result<Vec<Binding>, String> {
    let mut out = Vec::new();
    for (command, context, text) in defaults {
        let (key, applies) = parse_default(text, platform)?;
        if applies {
            out.push(Binding { command: command.to_string(), context, key });
        }
    }
    Ok(out)
}

impl Keymap {
    /// A keymap for this platform from default bindings `(command, context, key text)`; it knows
    /// the commands they name. Every key must parse.
    pub fn new<'a>(defaults: impl IntoIterator<Item = (&'a str, Context, &'a str)>) -> Result<Keymap, String> {
        Keymap::new_on(defaults, Platform::current())
    }

    /// The same, for `platform`.
    pub fn new_on<'a>(defaults: impl IntoIterator<Item = (&'a str, Context, &'a str)>, platform: Platform) -> Result<Keymap, String> {
        let defaults = bindings_on(defaults, platform)?;
        let mut commands: Vec<String> = Vec::new();
        for b in &defaults {
            if !commands.contains(&b.command) {
                commands.push(b.command.clone());
            }
        }
        Ok(Keymap { commands, defaults, overrides: Vec::new() })
    }

    /// The defaults declared by every command spec, for this platform; it knows every spec.
    pub fn from_specs(specs: &[crate::registry::CommandSpec]) -> Result<Keymap, String> {
        Keymap::from_specs_on(specs, Platform::current())
    }

    /// The same, for `platform`.
    pub fn from_specs_on(specs: &[crate::registry::CommandSpec], platform: Platform) -> Result<Keymap, String> {
        let mut map = Keymap::default();
        map.add_commands(specs.iter().map(|s| (s.id, s.keys)), platform)?;
        Ok(map)
    }

    /// Adds commands (the UI's) with their default keys for `platform`. A command it already knows
    /// has its defaults replaced, so adding the same list twice changes nothing. Nothing changes
    /// when a key does not parse. Overrides are kept.
    pub fn add_commands<'a>(&mut self, commands: impl IntoIterator<Item = (&'a str, DefaultKeys)>, platform: Platform) -> Result<(), String> {
        let commands: Vec<(&str, DefaultKeys)> = commands.into_iter().collect();
        let added = bindings_on(commands.iter().flat_map(|(id, keys)| keys.iter().map(move |(ctx, k)| (*id, *ctx, *k))), platform)?;
        self.defaults.retain(|b| !commands.iter().any(|(id, _)| *id == b.command));
        self.defaults.extend(added);
        for (id, _) in commands {
            if !self.knows(id) {
                self.commands.push(id.to_string());
            }
        }
        Ok(())
    }

    /// Whether `command` may be bound.
    pub fn knows(&self, command: &str) -> bool {
        self.commands.iter().any(|c| c == command)
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
