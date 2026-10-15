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
        if text.len() > MAX_KEY_TEXT {
            return Err(bad());
        }
        let mut parts: Vec<&str> = text.split('+').collect();
        // `Ctrl++` / `Ctrl+Plus`: a trailing empty part means the key itself is `+`.
        if text.ends_with("++") {
            parts.truncate(parts.len().saturating_sub(2));
            parts.push("Plus");
        }
        let (code, mods) = parts.split_last().ok_or_else(bad)?;
        Key::from_parts(mods, code).ok_or_else(bad)
    }

    /// [`Key::parse`], and also the spellings people type or read on screen: `-` or spaces
    /// between the parts (`ctrl-1`, `Ctrl 1`) and the macOS symbols [`Key::pretty`] writes
    /// (`⇧⌘S`, `⌥↩`). The search of `keymap.list` and `app.setShortcut` take keys this way.
    pub fn parse_lenient(text: &str) -> Result<Key, String> {
        let bad = || format!("`{text}` is not a key");
        let text = text.trim();
        if let Ok(key) = Key::parse(text) {
            return Ok(key);
        }
        if text.len() > MAX_KEY_TEXT || text.is_empty() {
            return Err(bad());
        }
        // macOS symbols: the modifiers first, then the key (a name or its symbol).
        let rest = text.trim_start_matches(['⌃', '⌥', '⇧', '⌘']);
        let symbols = text.strip_suffix(rest).unwrap_or_default();
        if !symbols.is_empty() {
            let mods: Vec<&str> = symbols
                .chars()
                .map(|c| match c {
                    '⌥' => "Alt",
                    '⇧' => "Shift",
                    _ => "Ctrl",
                })
                .collect();
            let code = rest.trim();
            return Key::from_parts(&mods, from_symbol(code).unwrap_or(code)).ok_or_else(bad);
        }
        if let Some(code) = from_symbol(text) {
            return Key::from_parts(&[], code).ok_or_else(bad);
        }
        // `-` or spaces between the parts; a separator as the last character after another is
        // the key itself (`ctrl -`, `ctrl--`).
        let is_sep = |c: char| c == '-' || c == '+' || c.is_whitespace();
        let (body, code) = match text.strip_suffix(['-', '+']).filter(|body| body.ends_with(is_sep)) {
            Some(body) => (body, if text.ends_with('+') { "Plus" } else { "-" }),
            None => match text.rsplit_once(is_sep) {
                Some((body, code)) => (body, code),
                None => ("", text),
            },
        };
        let mods: Vec<&str> = body.split(is_sep).filter(|m| !m.is_empty()).collect();
        Key::from_parts(&mods, code).ok_or_else(bad)
    }

    /// The key from its modifier names and its key name; `None` when one is not a key's.
    fn from_parts(mods: &[&str], code: &str) -> Option<Key> {
        let mut key = Key { ctrl: false, alt: false, shift: false, code: String::new() };
        for m in mods {
            let flag = match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "control" | "command" => &mut key.ctrl,
                "alt" | "option" | "opt" => &mut key.alt,
                "shift" => &mut key.shift,
                _ => return None,
            };
            if *flag {
                return None;
            }
            *flag = true;
        }
        key.code = canonical_code(code.trim())?;
        Some(key)
    }

    /// The key as the screen shows it on `platform`: `Ctrl+Shift+S` on Windows and Linux, `⇧⌘S`
    /// on macOS (Option ⌥, Shift ⇧, Command ⌘ in Apple's order, and the symbols of the named
    /// keys: `↩`, `⎋`, `⌫`, arrows …). [`Key::parse_lenient`] reads both back.
    pub fn pretty(&self, platform: Platform) -> String {
        if platform != Platform::Mac {
            return self.to_string();
        }
        let mut out = String::new();
        for (on, symbol) in [(self.alt, '⌥'), (self.shift, '⇧'), (self.ctrl, '⌘')] {
            if on {
                out.push(symbol);
            }
        }
        out.push_str(SYMBOLS.iter().find(|(name, _)| *name == self.code).map_or(self.code.as_str(), |(_, symbol)| symbol));
        out
    }
}

/// The longest key text taken (`Ctrl+Alt+Shift+PageDown` is 23 bytes).
const MAX_KEY_TEXT: usize = 64;

/// The macOS symbols of named keys ([`Key::pretty`]).
const SYMBOLS: [(&str, &str); 14] = [
    ("Enter", "↩"),
    ("Esc", "⎋"),
    ("Tab", "⇥"),
    ("Backspace", "⌫"),
    ("Delete", "⌦"),
    ("Home", "↖"),
    ("End", "↘"),
    ("PageUp", "⇞"),
    ("PageDown", "⇟"),
    ("Up", "↑"),
    ("Down", "↓"),
    ("Left", "←"),
    ("Right", "→"),
    ("Plus", "+"),
];

/// The key name of a macOS key symbol.
fn from_symbol(text: &str) -> Option<&'static str> {
    SYMBOLS.iter().find(|(_, symbol)| *symbol == text).map(|(name, _)| *name)
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

/// How many keys a command may have in one context (Star: Ctrl+8 and F4).
pub const KEYS_PER_CONTEXT: usize = 2;

/// Commands run by holding a key rather than pressing it (`menus-and-shortcuts.md` §2.0: the
/// quick full-screen preview's Ctrl+Alt, Show original's `\`): listed, never rebound. None is a
/// keymap command yet; their tasks add their ids here.
pub const HOLD_GESTURES: &[&str] = &[];

/// Whether `command` is a hold gesture ([`HOLD_GESTURES`]).
pub fn is_hold(command: &str) -> bool {
    HOLD_GESTURES.contains(&command)
}

/// Every context, in the order the editor's Context filter lists them.
pub const CONTEXTS: [Context; 8] = [
    Context::Global,
    Context::Library,
    Context::EditView,
    Context::Slideshow,
    Context::PhotoViewer,
    Context::Import,
    Context::Collage,
    Context::TextInput,
];

/// The name a context goes by on screen (`Edit view`).
pub fn context_label(context: Context) -> &'static str {
    match context {
        Context::Global => "Global",
        Context::Library => "Library",
        Context::EditView => "Edit view",
        Context::Slideshow => "Slideshow",
        Context::PhotoViewer => "Photo viewer",
        Context::Import => "Import",
        Context::Collage => "Collage",
        Context::TextInput => "Text input",
    }
}

/// Why [`Keymap::set`] refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetError {
    /// The key is another command's in a context active at the same time: that binding.
    Conflict(Binding),
    /// No such key slot (they are 0 and 1).
    Slot(usize),
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetError::Conflict(b) => write!(f, "{} is already {} in {}.", b.key, b.command, context_label(b.context)),
            SetError::Slot(n) => write!(f, "slot must be 0 or 1, not {n}"),
        }
    }
}

/// One row of the shortcut editor ([`Keymap::rows`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub command: String,
    /// `None` for a command without keys anywhere.
    pub context: Option<Context>,
    /// The keys in effect.
    pub keys: Vec<Key>,
    /// The default keys.
    pub defaults: Vec<Key>,
    /// Hold gestures are listed but not editable.
    pub editable: bool,
}

impl Row {
    /// Whether the keys in effect differ from the defaults (the editor's ↺).
    pub fn modified(&self) -> bool {
        self.keys != self.defaults
    }
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

    /// The binding of another command that `key` would clash with if `command` had it in
    /// `context`: the same key in a context that can be active at the same time.
    pub fn clash(&self, command: &str, context: Context, key: &Key) -> Option<Binding> {
        self.bindings().into_iter().find(|b| &b.key == key && overlapping(b.context, context) && b.command != command)
    }

    /// Binds `key` (or, with `None`, no key) to `command` in `context`. Without a `slot` it
    /// replaces the command's keys there; with one (0 or 1, [`KEYS_PER_CONTEXT`]) it sets or
    /// removes that key only, the other kept (a key set in a slot past the last one is added
    /// after it). A clash with another command is refused ([`SetError::Conflict`]), unless
    /// `replace`, which removes the key from the other command. Keys equal to the defaults are no
    /// override.
    pub fn set(&mut self, command: &str, context: Context, slot: Option<usize>, key: Option<Key>, replace: bool) -> Result<(), SetError> {
        if let Some(slot) = slot.filter(|s| *s >= KEYS_PER_CONTEXT) {
            return Err(SetError::Slot(slot));
        }
        if let Some(k) = &key {
            if let Some(c) = self.clash(command, context, k)
                && !replace
            {
                return Err(SetError::Conflict(c));
            }
            let clashes: Vec<Binding> =
                self.bindings().into_iter().filter(|b| &b.key == k && overlapping(b.context, context) && b.command != command).collect();
            for c in clashes {
                let keys = self.keys_for(&c.command, c.context).into_iter().filter(|x| x != k).collect();
                self.put(Override { command: c.command, context: c.context, keys });
            }
        }
        let keys = match slot {
            None => key.into_iter().collect(),
            Some(slot) => {
                let mut keys = self.keys_for(command, context);
                match key {
                    Some(k) => {
                        // A key the command already has in the other slot moves to this one.
                        if let Some(at) = keys.iter().position(|x| *x == k) {
                            keys.remove(at);
                        }
                        match keys.get_mut(slot) {
                            Some(old) => *old = k,
                            None => keys.push(k),
                        }
                    }
                    None if slot < keys.len() => {
                        keys.remove(slot);
                    }
                    None => {}
                }
                keys
            }
        };
        self.put(Override { command: command.to_string(), context, keys });
        Ok(())
    }

    /// Stores `o`, or drops the override when its keys are the defaults.
    fn put(&mut self, o: Override) {
        self.overrides.retain(|x| !(x.command == o.command && x.context == o.context));
        if o.keys != self.default_keys_for(&o.command, o.context) {
            self.overrides.push(o);
        }
    }

    /// The default keys of `command` in `context` on this keymap's platform.
    pub fn default_keys_for(&self, command: &str, context: Context) -> Vec<Key> {
        self.defaults.iter().filter(|b| b.command == command && b.context == context).map(|b| b.key.clone()).collect()
    }

    /// Every command that may be bound, in the order they joined.
    pub fn commands(&self) -> &[String] {
        &self.commands
    }

    /// The shortcut editor's rows (`keymap.list`): one per command and context it has keys in
    /// (by default or by the user, also when the user removed them all), contexts in their
    /// order; with `without_keys`, one row with no context for each command that has none.
    pub fn rows(&self, without_keys: bool) -> Vec<Row> {
        let bindings = self.bindings();
        let mut out = Vec::new();
        for command in &self.commands {
            let mut contexts: Vec<Context> = self
                .defaults
                .iter()
                .filter(|b| &b.command == command)
                .map(|b| b.context)
                .chain(self.overrides.iter().filter(|o| &o.command == command).map(|o| o.context))
                .collect();
            contexts.sort();
            contexts.dedup();
            if contexts.is_empty() && without_keys {
                out.push(Row { command: command.clone(), context: None, keys: Vec::new(), defaults: Vec::new(), editable: !is_hold(command) });
            }
            for context in contexts {
                let keys = bindings.iter().filter(|b| &b.command == command && b.context == context).map(|b| b.key.clone()).collect();
                out.push(Row {
                    command: command.clone(),
                    context: Some(context),
                    keys,
                    defaults: self.default_keys_for(command, context),
                    editable: !is_hold(command),
                });
            }
        }
        out
    }

    /// Drops the user's overrides for `command` (all contexts), or all of them.
    pub fn reset(&mut self, command: Option<&str>) {
        match command {
            Some(c) => self.overrides.retain(|o| o.command != c),
            None => self.overrides.clear(),
        }
    }
}
