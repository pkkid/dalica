//! The input methods (`control-protocol.md` §6.8, §6.9, §6.12): each checks everything first, then
//! plays real egui events; the reply comes after the input has played out (§5.2).

use dalica_engine::Key;
use dalica_ui_kit::{WidgetInfo, registry};
use egui::{Modifiers, PointerButton, Pos2, Vec2};
use serde::Deserialize;
use serde_json::{Value, json};

use super::host::{DragTo, Input, Played};
use super::{
    Call, Code, DEFAULT_SETTLE_MS, Error, MAX_CLICK_COUNT, MAX_COORD, MAX_DRAG_STEPS, MAX_POINTER_EVENTS, MAX_TEXT_BYTES, MAX_WAIT_MS, Outcome,
};
use crate::headless::{Click, Drag, KeyState, PointerEvent, PointerKind, modifiers};

/// The largest window edge in pixels and the most pixels (what the rasterizer paints).
const MAX_WINDOW_SIDE: f64 = dalica_ui_kit::raster::MAX_SIDE as f64;
const MAX_WINDOW_PIXELS: f64 = dalica_ui_kit::raster::MAX_PIXELS as f64;

/// The modifiers of the flags most input methods take. `ctrl` and `cmd` are both the command
/// modifier (Command on macOS), as the docs write shortcuts.
fn mods(ctrl: bool, cmd: bool, shift: bool, alt: bool) -> Modifiers {
    modifiers(ctrl || cmd, shift, alt, false)
}

fn button(call: &Call<'_>, name: Option<&str>) -> Result<PointerButton, Error> {
    match name.unwrap_or("left") {
        "left" | "primary" => Ok(PointerButton::Primary),
        "right" | "secondary" => Ok(PointerButton::Secondary),
        "middle" => Ok(PointerButton::Middle),
        other => Err(call.bad(format!("`button` is left, right or middle, not `{other}`"))),
    }
}

/// A fraction of a widget's size, 0..=1.
fn fraction(call: &Call<'_>, name: &str, v: Option<f64>) -> Result<f32, Error> {
    let v = v.unwrap_or(0.5);
    if !(0.0..=1.0).contains(&v) {
        return Err(call.bad(format!("`{name}` is a fraction 0 … 1 of the widget, not {v}")));
    }
    Ok(v as f32)
}

/// A screen coordinate or offset in points.
fn coord(call: &Call<'_>, name: &str, v: f64) -> Result<f32, Error> {
    if !(v.is_finite() && v.abs() <= MAX_COORD) {
        return Err(call.bad(format!("`{name}` is a point within ±{MAX_COORD}, not {v}")));
    }
    Ok(v as f32)
}

/// The widget `id` in the last frame: `not_found` if absent.
fn widget(call: &Call<'_>, id: &str) -> Result<WidgetInfo, Error> {
    registry::find(call.host.ctx(), id).ok_or_else(|| Error::new(Code::NotFound, format!("no widget `{id}` on screen (see ui.widgets)")))
}

/// The same, which must be enabled: `disabled` if it is not.
fn enabled_widget(call: &Call<'_>, id: &str) -> Result<WidgetInfo, Error> {
    let w = widget(call, id)?;
    if !w.enabled {
        return Err(Error::new(Code::Disabled, format!("widget `{id}` is disabled")));
    }
    Ok(w)
}

fn limit(what: String) -> Error {
    Error::new(Code::Limit, what)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClickWidget {
    id: String,
    count: Option<u64>,
    button: Option<String>,
    fx: Option<f64>,
    fy: Option<f64>,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    cmd: bool,
}

/// `ui.clickWidget`: clicks a widget (§6.8); replies its rect.
pub(super) fn click_widget(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: ClickWidget = call.params(params)?;
    let count = p.count.unwrap_or(1);
    if !(1..=MAX_CLICK_COUNT).contains(&count) {
        return Err(limit(format!("`count` is 1 … {MAX_CLICK_COUNT}, not {count}")));
    }
    let (fx, fy) = (fraction(call, "fx", p.fx)?, fraction(call, "fy", p.fy)?);
    let button = button(call, p.button.as_deref())?;
    let w = enabled_widget(call, &p.id)?;
    // A dialog button automation never presses (`button:dialog.remove` of Remove from Card).
    super::dialog::never_automated_button(call.host.app(), p.id.strip_prefix("button:dialog."))?;
    let click = Click { fx, fy, count: count as u32, button, modifiers: mods(p.ctrl, p.cmd, p.shift, p.alt) };
    Ok(Outcome::play_then_reply(Input::Click { id: p.id, click }, json!({ "rect": w.rect })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DragWidget {
    id: String,
    to_widget: Option<String>,
    to_x: Option<f64>,
    to_y: Option<f64>,
    dx: Option<f64>,
    dy: Option<f64>,
    steps: Option<u64>,
    fx: Option<f64>,
    fy: Option<f64>,
    button: Option<String>,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    cmd: bool,
}

/// `ui.dragWidget`: drags from a widget onto another widget, a point, or by an offset (§6.8).
pub(super) fn drag_widget(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: DragWidget = call.params(params)?;
    let steps = p.steps.unwrap_or(12);
    if !(1..=MAX_DRAG_STEPS).contains(&steps) {
        return Err(limit(format!("`steps` is 1 … {MAX_DRAG_STEPS}, not {steps}")));
    }
    let (fx, fy) = (fraction(call, "fx", p.fx)?, fraction(call, "fy", p.fy)?);
    let button = button(call, p.button.as_deref())?;
    let to = match (p.to_widget, p.to_x, p.to_y, p.dx, p.dy) {
        (Some(id), None, None, None, None) => DragTo::Widget(id),
        (None, Some(x), Some(y), None, None) => DragTo::Point(Pos2::new(coord(call, "toX", x)?, coord(call, "toY", y)?)),
        (None, None, None, Some(dx), Some(dy)) => DragTo::Delta(Vec2::new(coord(call, "dx", dx)?, coord(call, "dy", dy)?)),
        _ => return Err(call.bad("give exactly one target: `toWidget`, `toX` and `toY`, or `dx` and `dy`")),
    };
    let from = enabled_widget(call, &p.id)?;
    let start = Pos2::new(from.rect[0] + from.rect[2] * fx, from.rect[1] + from.rect[3] * fy);
    let end = match &to {
        DragTo::Widget(id) => widget(call, id)?.center(),
        DragTo::Point(q) => *q,
        DragTo::Delta(v) => start + *v,
    };
    if !(end.x.abs() as f64 <= MAX_COORD && end.y.abs() as f64 <= MAX_COORD) {
        return Err(call.bad(format!("the drag ends outside ±{MAX_COORD} points")));
    }
    let drag = Drag { steps: steps as usize, button, modifiers: mods(p.ctrl, p.cmd, p.shift, p.alt) };
    let input = Input::Drag { from: p.id, at: (fx, fy), to, drag };
    Ok(Outcome::play_then_reply(input, json!({ "from": [start.x, start.y], "to": [end.x, end.y] })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HoverWidget {
    id: String,
    fx: Option<f64>,
    fy: Option<f64>,
    ms: Option<u64>,
}

/// `ui.hoverWidget`: rests the pointer on a widget (enabled or not), then lets `ms` pass.
pub(super) fn hover_widget(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: HoverWidget = call.params(params)?;
    let ms = p.ms.unwrap_or(0);
    if ms > MAX_WAIT_MS {
        return Err(limit(format!("`ms` is at most {MAX_WAIT_MS}, not {ms}")));
    }
    let at = (fraction(call, "fx", p.fx)?, fraction(call, "fy", p.fy)?);
    let w = widget(call, &p.id)?;
    Ok(Outcome::play_then_reply(Input::Hover { id: p.id, at, ms }, json!({ "rect": w.rect })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvent {
    kind: String,
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerParams {
    events: Vec<RawEvent>,
    space: Option<String>,
    button: Option<String>,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    cmd: bool,
}

/// `ui.pointer`: a raw gesture in screen points (§6.8).
pub(super) fn pointer(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    // The count first, so an over-long list is a `limit` whatever its contents.
    if let Some(n) = params.get("events").and_then(Value::as_array).map(Vec::len)
        && n > MAX_POINTER_EVENTS
    {
        return Err(limit(format!("at most {MAX_POINTER_EVENTS} events per request, not {n}")));
    }
    let p: PointerParams = call.params(params)?;
    match p.space.as_deref().unwrap_or("screen") {
        "screen" => {}
        "image" => {
            return Err(Error::new(Code::Unsupported, "`space: \"image\"` needs the edit view (M7); use screen points"));
        }
        other => return Err(call.bad(format!("`space` is screen or image, not `{other}`"))),
    }
    let button = button(call, p.button.as_deref())?;
    let mut events = Vec::with_capacity(p.events.len());
    for (i, e) in p.events.iter().enumerate() {
        let kind = match e.kind.as_str() {
            "move" => PointerKind::Move,
            "down" => PointerKind::Down,
            "drag" => PointerKind::Drag,
            "up" => PointerKind::Up,
            other => return Err(call.bad(format!("events[{i}].kind is move, down, drag or up, not `{other}`"))),
        };
        let pos = Pos2::new(coord(call, &format!("events[{i}].x"), e.x)?, coord(call, &format!("events[{i}].y"), e.y)?);
        events.push(PointerEvent { kind, pos });
    }
    let n = events.len();
    Ok(Outcome::play_then_reply(Input::Pointer { events, button, modifiers: mods(p.ctrl, p.cmd, p.shift, p.alt) }, json!({ "events": n })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KeyParams {
    key: Option<String>,
    combo: Option<String>,
    state: Option<String>,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    cmd: bool,
    #[serde(default)]
    mac_ctrl: bool,
}

/// egui's key for a name: egui's own names (`A`, `Enter`, `ArrowLeft`, `F2`, `Backslash`, …), the
/// keymap's (`Esc`, `Up`, `Plus`), and lower-case aliases (`esc`, `return`, `left`, `space`).
fn key_named(name: &str) -> Option<egui::Key> {
    let capitalised = |s: &str| {
        let mut c = s.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
    };
    egui::Key::from_name(name).or_else(|| egui::Key::from_name(&capitalised(name)))
}

/// `ui.key {key | combo, state?, …}`: presses (or holds, or releases) a key (§6.9). `combo` uses
/// the shortcut syntax, where `Ctrl` is the command modifier; `key` takes the modifier flags.
pub(super) fn key(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: KeyParams = call.params(params)?;
    let state = match p.state.as_deref().unwrap_or("press") {
        "press" => KeyState::Press,
        "down" => KeyState::Down,
        "up" => KeyState::Up,
        other => return Err(call.bad(format!("`state` is press, down or up, not `{other}`"))),
    };
    let flags = p.ctrl || p.cmd || p.shift || p.alt || p.mac_ctrl;
    let (key, mods) = match (p.key, p.combo) {
        (Some(name), None) => {
            let key = key_named(&name).ok_or_else(|| call.bad(format!("no key `{name}` (egui names such as A, Enter, Escape, ArrowLeft, F2)")))?;
            (key, modifiers(p.ctrl || p.cmd, p.shift, p.alt, p.mac_ctrl))
        }
        (None, Some(combo)) if !flags => {
            let k = Key::parse(&combo).map_err(|e| call.bad(e))?;
            let key = key_named(&k.code).ok_or_else(|| call.bad(format!("`{combo}` has no key egui can press")))?;
            (key, modifiers(k.ctrl, k.shift, k.alt, false))
        }
        (None, Some(_)) => return Err(call.bad("with `combo`, write the modifiers in it (`Ctrl+Shift+R`); the flags go with `key`")),
        _ => return Err(call.bad("give either `key` or `combo`")),
    };
    match state {
        KeyState::Down => call.conn.hold(key, mods),
        KeyState::Up => call.conn.let_go(key),
        KeyState::Press => {}
    }
    Ok(Outcome::play_then_reply(Input::Key { key, modifiers: mods, state }, json!({})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeParams {
    text: String,
}

/// `ui.type {text}`: types into the focused widget.
pub(super) fn type_text(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    if let Some(n) = params.get("text").and_then(Value::as_str).map(str::len)
        && n > MAX_TEXT_BYTES
    {
        return Err(limit(format!("`text` is at most {MAX_TEXT_BYTES} bytes, not {n}")));
    }
    let p: TypeParams = call.params(params)?;
    if p.text.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(call.bad("`text` has control characters other than newline and tab; press their keys with ui.key"));
    }
    Ok(Outcome::play_then_reply(Input::Text(p.text), json!({})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScrollParams {
    dx: Option<f64>,
    dy: Option<f64>,
    widget: Option<String>,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    alt: bool,
}

/// `ui.scroll {dx?, dy?, widget?}`: a wheel scroll over a widget or where the pointer is. Positive
/// `dy` reveals what is above.
pub(super) fn scroll(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: ScrollParams = call.params(params)?;
    if p.ctrl || p.shift || p.alt {
        return Err(Error::new(
            Code::Unsupported,
            "scrolling with modifier flags is not built yet; hold the modifier with ui.key {combo, state: \"down\"} first",
        ));
    }
    let delta = Vec2::new(coord(call, "dx", p.dx.unwrap_or(0.0))?, coord(call, "dy", p.dy.unwrap_or(0.0))?);
    if let Some(id) = &p.widget {
        widget(call, id)?;
    }
    Ok(Outcome::play_then_reply(Input::Scroll { at: p.widget, delta }, json!({})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResizeParams {
    width: f64,
    height: f64,
}

/// `ui.resize {width, height}` in points: the window's size (the headless one; a real window is
/// resized by its transport). At most what the rasterizer paints.
pub(super) fn resize(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: ResizeParams = call.params(params)?;
    let ppp = f64::from(call.host.window().pixels_per_point);
    let (w, h) = (p.width * ppp, p.height * ppp);
    if !(w.is_finite() && h.is_finite()) {
        return Err(call.bad("`width` and `height` are numbers of points"));
    }
    if !(w.round() >= 1.0 && h.round() >= 1.0 && w <= MAX_WINDOW_SIDE && h <= MAX_WINDOW_SIDE && w.round() * h.round() <= MAX_WINDOW_PIXELS) {
        return Err(limit(format!(
            "a window of {} × {} points is {w} × {h} pixels: each side 1 … {MAX_WINDOW_SIDE}, at most {MAX_WINDOW_PIXELS} pixels",
            p.width, p.height
        )));
    }
    let (width, height) = (p.width as f32, p.height as f32);
    Ok(Outcome::Play(
        Input::Resize { width, height },
        Box::new(|host, played| match played {
            Err(e) => Outcome::Reply(Err(e.into())),
            Ok(_) => {
                let w = host.window();
                Outcome::Reply(Ok(json!({ "width": w.width, "height": w.height })))
            }
        }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitParams {
    ms: u64,
}

/// `ui.wait {ms}`: lets time pass (virtual time, headless).
pub(super) fn wait(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: WaitParams = call.params(params)?;
    if p.ms > MAX_WAIT_MS {
        return Err(limit(format!("`ms` is at most {MAX_WAIT_MS}, not {}", p.ms)));
    }
    Ok(Outcome::Play(
        Input::Wait { ms: p.ms },
        Box::new(|_, played| match played {
            Ok(Played::Frames(n)) => Outcome::Reply(Ok(json!({ "frames": n }))),
            Ok(_) => Outcome::Reply(Ok(json!({ "frames": null }))),
            Err(e) => Outcome::Reply(Err(e.into())),
        }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SettleParams {
    timeout_ms: Option<u64>,
    #[serde(default)]
    jobs: bool,
}

/// `ui.settle {timeoutMs?, jobs?}`: frames until nothing visible is pending (§6.12).
pub(super) fn settle(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: SettleParams = call.params(params)?;
    let timeout_ms = p.timeout_ms.unwrap_or(DEFAULT_SETTLE_MS);
    if timeout_ms > MAX_WAIT_MS {
        return Err(limit(format!("`timeoutMs` is at most {MAX_WAIT_MS}, not {timeout_ms}")));
    }
    Ok(Outcome::Play(
        Input::Settle { timeout_ms, jobs: p.jobs },
        Box::new(|_, played| match played {
            Ok(Played::Settled(s)) => Outcome::Reply(serde_json::to_value(s).map_err(|e| Error::new(Code::Internal, e.to_string()))),
            Ok(other) => Outcome::Reply(Err(Error::new(Code::Internal, format!("a settle played as {other:?}")))),
            Err(e) => Outcome::Reply(Err(e.into())),
        }),
    ))
}
