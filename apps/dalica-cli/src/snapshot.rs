//! `dalica-cli snapshot`: a screenshot of the app without a window (`architecture.md` §7). It opens
//! the library, drives the real UI headlessly (an optional JSON-lines script of clicks, keys and
//! commands), waits for background work, paints the last frame with the CPU rasterizer and writes a
//! PNG. Writes the library made are flushed before exiting.
//!
//! Script lines: `{"click": widgetId}`, `{"key": "Ctrl+8"}`, `{"command": id, "params": {...}}`,
//! `{"settle": true}`; blank lines and lines starting with `#` are skipped.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// Default window size (`docs/ui-layout.md` §11) and the largest accepted.
const DEFAULT_SIZE: (u32, u32) = (1400, 900);
const MAX_SIDE: u32 = 8192;
/// The longest script accepted, and the frames to wait for background work per settle.
const MAX_SCRIPT: u64 = 16 << 20;
const SETTLE_FRAMES: usize = 60 * 60 * 5;

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// `WIDTHxHEIGHT`, each 1 ..= 8192.
fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once(['x', 'X', '×'])?;
    let (w, h) = (w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?);
    ((1..=MAX_SIDE).contains(&w) && (1..=MAX_SIDE).contains(&h)).then_some((w, h))
}

fn bad(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("dalica-cli snapshot: {msg}");
    ExitCode::from(2)
}

pub fn main(args: &[String]) -> ExitCode {
    let Some(dir) = crate::library_arg(args) else { return bad("--library DIR is required") };
    let Some(out) = flag(args, "-o").or_else(|| flag(args, "--output")).map(PathBuf::from) else { return bad("-o OUT.png is required") };
    let size = match flag(args, "--size") {
        None => DEFAULT_SIZE,
        Some(s) => match parse_size(s) {
            Some(v) => v,
            None => return bad(format!("--size `{s}` is not WIDTHxHEIGHT with sides 1 … {MAX_SIDE}")),
        },
    };
    let scale = match flag(args, "--scale").map(str::parse::<f32>) {
        None => 1.0,
        Some(Ok(v)) if (0.5..=4.0).contains(&v) => v,
        Some(_) => return bad("--scale must be a number from 0.5 to 4"),
    };
    let script = match flag(args, "--script") {
        None => Vec::new(),
        Some(path) => match read_script(Path::new(path)) {
            Ok(lines) => lines,
            Err(e) => return bad(e),
        },
    };
    let started = Instant::now();
    let session = match crate::open(&dir) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let mut driver = Driver::new(App::new(Some(session)), size.0, size.1, scale);
    let mut failed = None;
    if let Err(e) = driver.settle(SETTLE_FRAMES) {
        failed = Some(format!("the library did not settle: {e}"));
    }
    for (n, line) in script.iter().enumerate() {
        if failed.is_some() {
            break;
        }
        if let Err(e) = step(&mut driver, line) {
            failed = Some(format!("script line {}: {e}", n + 1));
        }
    }
    if failed.is_none()
        && let Err(e) = driver.settle(SETTLE_FRAMES)
    {
        failed = Some(format!("the library did not settle: {e}"));
    }
    let frames = driver.frames();
    let shot = driver.screenshot();
    let mut app = driver.into_app();
    if let Some(Err(e)) = app.session_mut().map(dalica_engine::Session::flush) {
        failed.get_or_insert(format!("not all changes were saved: {e}"));
    }
    drop(app);
    if let Some(why) = failed {
        eprintln!("dalica-cli snapshot: {why}");
        return ExitCode::from(1);
    }
    let shot = match shot {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dalica-cli snapshot: {e}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = write_png(&out, shot.width, shot.height, &shot.rgba) {
        eprintln!("dalica-cli snapshot: cannot write {}: {e}", out.display());
        return ExitCode::from(1);
    }
    let summary = json!({
        "ok": true, "path": out, "width": shot.width, "height": shot.height, "scale": scale,
        "frames": frames, "ms": started.elapsed().as_millis() as u64,
    });
    println!("{summary}");
    ExitCode::SUCCESS
}

fn read_script(path: &Path) -> Result<Vec<Value>, String> {
    let read = dalica_fs::read(path, MAX_SCRIPT).map_err(|e| e.to_string())?.ok_or_else(|| format!("no script {}", path.display()))?;
    let text = String::from_utf8(read.bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;
    let mut lines = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let v: Value = serde_json::from_str(line).map_err(|e| format!("script line {}: {e}", n + 1))?;
        if !v.is_object() {
            return Err(format!("script line {}: not a JSON object", n + 1));
        }
        lines.push(v);
    }
    Ok(lines)
}

fn step(d: &mut Driver, line: &Value) -> Result<(), String> {
    if let Some(id) = line.get("click").and_then(Value::as_str) {
        return d.click(id).map_err(|e| e.to_string());
    }
    if let Some(key) = line.get("key").and_then(Value::as_str) {
        return d.key(key).map_err(|e| e.to_string());
    }
    if let Some(id) = line.get("command").and_then(Value::as_str) {
        let params = line.get("params").cloned().unwrap_or_else(|| json!({}));
        return d.run(id, &params).map(|_| ()).map_err(|e| e.to_string());
    }
    if line.get("settle").is_some() {
        return d.settle(SETTLE_FRAMES).map(|_| ()).map_err(|e| e.to_string());
    }
    Err(format!("unknown step {line}: expected click, key, command or settle"))
}

/// Encodes RGBA as PNG and writes it atomically (replacing an existing file at that path).
fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let mut bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut bytes, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(rgba).and_then(|()| w.finish())).map_err(|e| e.to_string())?;
    let expect = match dalica_fs::identity(path).map_err(|e| e.to_string())? {
        Some(id) => dalica_fs::Expect::Unchanged(id),
        None => dalica_fs::Expect::Absent,
    };
    dalica_fs::replace(path, &bytes, expect).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn png_size(path: &Path) -> (u32, u32) {
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
        let reader = decoder.read_info().unwrap();
        let info = reader.info();
        (info.width, info.height)
    }

    /// The M1.17 done-when: `snapshot --library DIR -o out.png --size 1400x900` writes that PNG.
    #[test]
    fn snapshot_writes_a_png_of_the_requested_size() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, out) = (dir.path().join("lib"), dir.path().join("out.png"));
        let (lib_s, out_s) = (lib.to_str().unwrap(), out.to_str().unwrap());
        assert_eq!(main(&args(&["--library", lib_s, "-o", out_s, "--size", "1400x900"])), ExitCode::SUCCESS);
        assert_eq!(png_size(&out), (1400, 900));
        // Again at another size, over the first file, with a script.
        let script = dir.path().join("steps.jsonl");
        std::fs::write(&script, "# a comment\n{\"key\": \"Ctrl+8\"}\n{\"command\": \"select.none\"}\n\n{\"settle\": true}\n").unwrap();
        let code = main(&args(&["--library", lib_s, "-o", out_s, "--size", "900x600", "--scale", "1.5", "--script", script.to_str().unwrap()]));
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(png_size(&out), (900, 600));
    }

    #[test]
    fn bad_arguments_and_failing_scripts_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (lib, out) = (dir.path().join("lib"), dir.path().join("out.png"));
        let (lib_s, out_s) = (lib.to_str().unwrap(), out.to_str().unwrap());
        for bad in [
            args(&["-o", out_s]),
            args(&["--library", lib_s]),
            args(&["--library", lib_s, "-o", out_s, "--size", "99999x10"]),
            args(&["--library", lib_s, "-o", out_s, "--scale", "nan"]),
            args(&["--library", lib_s, "-o", out_s, "--script", dir.path().join("missing.jsonl").to_str().unwrap()]),
        ] {
            assert_eq!(main(&bad), ExitCode::from(2), "{bad:?}");
        }
        for (name, text) in [("notjson", "{nope\n"), ("array", "[1]\n")] {
            let script = dir.path().join(name);
            std::fs::write(&script, text).unwrap();
            assert_eq!(main(&args(&["--library", lib_s, "-o", out_s, "--script", script.to_str().unwrap()])), ExitCode::from(2), "{name}");
        }
        for (name, text) in
            [("click", "{\"click\": \"no.such.widget\"}\n"), ("unknown", "{\"jump\": 1}\n"), ("command", "{\"command\": \"no.such\"}\n")]
        {
            let script = dir.path().join(name);
            std::fs::write(&script, text).unwrap();
            assert_eq!(main(&args(&["--library", lib_s, "-o", out_s, "--script", script.to_str().unwrap()])), ExitCode::from(1), "{name}");
        }
        assert!(!out.exists(), "nothing written on failure");
    }

    #[test]
    fn sizes_parse_within_bounds() {
        assert_eq!(parse_size("1400x900"), Some((1400, 900)));
        assert_eq!(parse_size("900×600"), Some((900, 600)));
        assert_eq!(parse_size("8192x8192"), Some((8192, 8192)), "the rasterizer refuses this many pixels: exit 1, not a crash");
        for bad in ["", "1400", "0x10", "10x0", "8193x10", "-1x5", "axb", "1x1x1"] {
            assert_eq!(parse_size(bad), None, "{bad}");
        }
    }
}
