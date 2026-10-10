//! Logging and the panic hook (`docs/development.md` §10).
//!
//! Records go to standard error and to `logs/dalica.log` in the settings folder, written through
//! `dalica-fs` (layer rule 4) by a writer thread, so logging never touches the disk on the caller's
//! thread (the UI thread must not, `architecture.md` §5). Each launch rotates the previous logs to
//! `dalica.1.log` and `dalica.2.log`; the file stops at 16 MiB with a closing notice. A panic is
//! also appended to `dalica-panics.log` in the OS temp folder, which survives an unwritable settings
//! folder. Nothing is written before [`init`], so `--version`, `--help` and argument errors leave
//! no file.
//!
//! Levels: Dalica's own crates at `info`, everything else at `warn`. `RUST_LOG` replaces that with
//! env_logger-style directives: a bare level (`debug`), `target=level` (matching the target and
//! its `::` children), `prefix*=level`; the longest matching target wins. Without a bare level,
//! targets no directive matches log errors only.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use dalica_core::{DateTime, Timestamp};
use log::{LevelFilter, Log, Metadata, Record};

/// The log file's size cap (`development.md` §10).
pub const LOG_CAP: u64 = 16 * 1024 * 1024;
/// Earlier runs kept: `dalica.1.log`, `dalica.2.log`.
pub const KEEP: usize = 2;
pub const LOG_FILE: &str = "dalica.log";
pub const PANICS_FILE: &str = "dalica-panics.log";
pub const PANICS_CAP: u64 = 4 * 1024 * 1024;
const STOP_NOTICE: &str = "-- dalica.log stopped here: it reached its 16 MiB cap --\n";
/// The longest a flush waits for the writer (a panic on the writer thread must not hang the hook).
const FLUSH_WAIT: Duration = Duration::from_secs(2);

/// Dalica's settings folder: Linux `$XDG_CONFIG_HOME/dalica` (default `~/.config/dalica`), macOS
/// `~/Library/Application Support/Dalica`, Windows `%APPDATA%\Dalica`. `None` without a home.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(|k| std::env::var_os(k))
}

fn config_dir_from(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let abs = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
    if cfg!(windows) {
        abs("APPDATA").map(|p| p.join("Dalica"))
    } else if cfg!(target_os = "macos") {
        abs("HOME").map(|h| h.join("Library").join("Application Support").join("Dalica"))
    } else {
        abs("XDG_CONFIG_HOME").or_else(|| abs("HOME").map(|h| h.join(".config"))).map(|p| p.join("dalica"))
    }
}

/// Which records pass: per-target levels from `RUST_LOG` or the defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    default: LevelFilter,
    /// (target, prefix match, level).
    directives: Vec<(String, bool, LevelFilter)>,
}

impl Filter {
    /// Dalica's crates at `info`, everything else at `warn`.
    pub fn defaults() -> Filter {
        Filter { default: LevelFilter::Warn, directives: vec![("dalica".into(), true, LevelFilter::Info)] }
    }

    /// The filter for a `RUST_LOG` value (unset or blank: the defaults), and the parts it ignored.
    pub fn from_env(value: Option<&str>) -> (Filter, Vec<String>) {
        match value.map(str::trim) {
            None | Some("") => (Filter::defaults(), Vec::new()),
            Some(spec) => Filter::parse(spec),
        }
    }

    fn parse(spec: &str) -> (Filter, Vec<String>) {
        let mut f = Filter { default: LevelFilter::Error, directives: Vec::new() };
        let mut ignored = Vec::new();
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (target, level) = match part.split_once('=') {
                Some((t, l)) => match LevelFilter::from_str(l.trim()) {
                    Ok(level) => (t.trim(), level),
                    Err(_) => {
                        ignored.push(part.to_string());
                        continue;
                    }
                },
                None => match LevelFilter::from_str(part) {
                    Ok(level) => {
                        f.default = level;
                        continue;
                    }
                    // A bare name is a target at every level, as in env_logger.
                    Err(_) => (part, LevelFilter::Trace),
                },
            };
            let (target, prefix) = match target.strip_suffix('*') {
                Some(t) => (t, true),
                None => (target, false),
            };
            if target.is_empty() || !target.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == '-') {
                ignored.push(part.to_string());
                continue;
            }
            f.directives.push((target.to_string(), prefix, level));
        }
        (f, ignored)
    }

    /// The level for records of `target`.
    pub fn level_for(&self, target: &str) -> LevelFilter {
        let matches = |(t, prefix, _): &&(String, bool, LevelFilter)| {
            if *prefix { target.starts_with(t.as_str()) } else { target == t || target.strip_prefix(t.as_str()).is_some_and(|r| r.starts_with("::")) }
        };
        self.directives.iter().filter(matches).max_by_key(|(t, _, _)| t.len()).map_or(self.default, |(_, _, l)| *l)
    }

    /// The most verbose level any target gets.
    pub fn max(&self) -> LevelFilter {
        self.directives.iter().map(|(_, _, l)| *l).chain([self.default]).max().unwrap_or(LevelFilter::Off)
    }
}

enum Msg {
    Line(String),
    Flush(Sender<()>),
}

/// The log file of this run: rotated at start, written by its own thread.
pub struct FileLog {
    tx: Option<Sender<Msg>>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl FileLog {
    /// Rotates earlier runs' logs in `logs_dir` (created if missing) and starts the writer.
    pub fn start(logs_dir: &Path) -> Result<FileLog, String> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let dir = logs_dir.to_path_buf();
        let thread = std::thread::Builder::new().name("dalica-log".into()).spawn(move || writer(&dir, rx, &ready_tx)).map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(FLUSH_WAIT) {
            Ok(Ok(path)) => Ok(FileLog { tx: Some(tx), thread: Some(thread), path }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the log writer did not start".into()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Queues one line (ending in a newline).
    pub fn write(&self, line: String) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Line(line));
        }
    }

    /// Waits (briefly) until every queued line is on disk; false if the writer did not answer.
    pub fn flush(&self) -> bool {
        let (ack_tx, ack_rx) = mpsc::channel();
        match &self.tx {
            Some(tx) if tx.send(Msg::Flush(ack_tx)).is_ok() => ack_rx.recv_timeout(FLUSH_WAIT).is_ok(),
            _ => false,
        }
    }
}

impl Drop for FileLog {
    /// Writes out the queue and stops the writer.
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn writer(dir: &Path, rx: Receiver<Msg>, ready: &Sender<Result<PathBuf, String>>) {
    let path = dir.join(LOG_FILE);
    let opened =
        dalica_fs::create_dir_all(dir).and_then(|()| dalica_fs::rotate(&path, KEEP)).and_then(|()| dalica_fs::Appender::open(&path, LOG_CAP));
    let mut file = match opened {
        Ok(a) => {
            let _ = ready.send(Ok(path));
            a
        }
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    let room = LOG_CAP.saturating_sub(STOP_NOTICE.len() as u64);
    let mut stopped = false;
    for msg in rx {
        match msg {
            Msg::Line(line) if !stopped => {
                if file.size().saturating_add(line.len() as u64) > room {
                    let _ = file.append(STOP_NOTICE.as_bytes());
                    stopped = true;
                } else if let Err(e) = file.append(line.as_bytes()) {
                    eprintln!("dalica: the log file stopped: {e}");
                    stopped = true;
                }
            }
            Msg::Line(_) => {}
            Msg::Flush(ack) => {
                let _ = ack.send(());
            }
        }
    }
}

/// `2026-06-04T19:42:10.123Z`, UTC.
fn timestamp(t: SystemTime) -> String {
    let ts = Timestamp::from_system_time(t);
    match DateTime::from_unix_secs(ts.as_secs()) {
        Some(dt) => format!("{dt}.{:03}Z", ts.as_millis().rem_euclid(1000)),
        None => format!("{}ms", ts.as_millis()),
    }
}

fn line(level: impl std::fmt::Display, target: &str, message: impl std::fmt::Display) -> String {
    format!("{} {level:<5} {target}: {message}\n", timestamp(SystemTime::now()))
}

struct Logger {
    filter: Filter,
    file: Option<FileLog>,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let text = line(record.level(), record.target(), record.args());
        eprint!("{text}");
        if let Some(file) = &self.file {
            file.write(text);
        }
    }

    fn flush(&self) {
        if let Some(file) = &self.file {
            file.flush();
        }
    }
}

static LOGGER: OnceLock<&'static Logger> = OnceLock::new();

/// Flushes the log file when dropped: keep it alive in `main` so the last records reach the disk.
#[must_use = "the log is flushed when this guard is dropped"]
pub struct LogGuard;

impl Drop for LogGuard {
    fn drop(&mut self) {
        log::logger().flush();
    }
}

/// Installs the logger (stderr + `<settings>/logs/dalica.log`) and the panic hook.
pub fn init() -> LogGuard {
    init_at(config_dir().map(|c| c.join("logs")))
}

/// [`init`] with an explicit logs folder (`None`: standard error only).
fn init_at(logs_dir: Option<PathBuf>) -> LogGuard {
    let (filter, ignored) = Filter::from_env(std::env::var("RUST_LOG").ok().as_deref());
    let file = logs_dir.and_then(|dir| match FileLog::start(&dir) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("dalica: no log file in {}: {e}", dir.display());
            None
        }
    });
    let max = filter.max();
    let logger: &'static Logger = Box::leak(Box::new(Logger { filter, file }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(max);
        let _ = LOGGER.set(logger);
    }
    if let Some(file) = LOGGER.get().and_then(|l| l.file.as_ref()) {
        log::info!("log file: {}", file.path().display());
    }
    for part in ignored {
        log::warn!("RUST_LOG: ignored `{part}`");
    }
    install_panic_hook();
    LogGuard
}

/// After the default hook's message: the panic's message, location and backtrace go to the log file
/// (bypassing the filter) and to `dalica-panics.log` in the OS temp folder.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let report = format!("panic in thread `{thread}` at {location}: {message}\n{}", std::backtrace::Backtrace::capture().to_string().trim_end());
        if let Some(file) = LOGGER.get().and_then(|l| l.file.as_ref()) {
            file.write(line("ERROR", "panic", &report));
            file.flush();
        }
        let entry = format!("{} dalica {} (pid {}): {report}\n", timestamp(SystemTime::now()), env!("CARGO_PKG_VERSION"), std::process::id());
        append_panics_file(&std::env::temp_dir().join(PANICS_FILE), entry.as_bytes());
    }));
}

/// Appends from a short-lived thread: the panicking thread may be the UI thread, where `fs` refuses.
fn append_panics_file(path: &Path, bytes: &[u8]) {
    std::thread::scope(|s| {
        if let Ok(h) = std::thread::Builder::new().name("dalica-panic-log".into()).spawn_scoped(s, || dalica_fs::append(path, bytes, PANICS_CAP)) {
            match h.join() {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => eprintln!("dalica: could not write {}: {e}", path.display()),
                Err(_) => eprintln!("dalica: could not write {}", path.display()),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Instant;

    use super::*;

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    /// How long a test waits for the writer to put its queue on disk. The writer syncs every
    /// record, so megabytes of writes can take longer than one flush waits ([`FLUSH_WAIT`]) on a
    /// busy machine.
    const FLUSHED_WITHIN: Duration = Duration::from_secs(30);

    /// Asserts every line queued so far reaches the disk within [`FLUSHED_WITHIN`]: flushes again
    /// while a flush gives up. The writer answers in queue order, so any flush's answer means
    /// every line queued before it was written. Returns how long it took.
    fn assert_flushed(log: &FileLog) -> Duration {
        let started = Instant::now();
        let mut tries = 0;
        loop {
            tries += 1;
            if log.flush() {
                return started.elapsed();
            }
            assert!(started.elapsed() < FLUSHED_WITHIN, "the log writer did not write its queue within {FLUSHED_WITHIN:?} ({tries} flushes gave up)");
            // A writer that is gone fails a flush at once: do not spin.
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn three_launches_rotate_and_keep_two_earlier_runs() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        for run in 1..=4 {
            let log = FileLog::start(&logs).unwrap();
            log.write(line("INFO", "dalica", format!("run {run}")));
            assert_flushed(&log);
            assert!(read(log.path()).contains(&format!("run {run}")));
        }
        assert!(read(&logs.join("dalica.log")).contains("run 4"));
        assert!(read(&logs.join("dalica.1.log")).contains("run 3"));
        assert!(read(&logs.join("dalica.2.log")).contains("run 2"));
        assert!(!logs.join("dalica.3.log").exists());
        let lines: Vec<String> = std::fs::read_dir(&logs).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
    }

    #[test]
    fn the_cap_holds_and_ends_with_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let log = FileLog::start(dir.path()).unwrap();
        let chunk = "x".repeat(1024 * 1024 - 1) + "\n";
        for _ in 0..20 {
            log.write(chunk.clone());
        }
        log.write("after the cap\n".into());
        let took = assert_flushed(&log);
        eprintln!("numbers: 20 records of 1 MiB capped and on disk {:.2} s after the last was queued", took.as_secs_f64());
        let len = std::fs::metadata(log.path()).unwrap().len();
        assert!(len <= LOG_CAP, "{len}");
        let text = read(log.path());
        assert!(text.ends_with(STOP_NOTICE));
        assert!(!text.contains("after the cap"));
    }

    #[test]
    fn an_unwritable_logs_folder_is_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("logs"), b"a file, not a folder").unwrap();
        assert!(FileLog::start(&dir.path().join("logs")).is_err());
    }

    #[test]
    fn rust_log_directives() {
        let (f, ignored) = Filter::from_env(None);
        assert_eq!(f, Filter::defaults());
        assert!(ignored.is_empty());
        assert_eq!(f.level_for("dalica_scan::walk"), LevelFilter::Info);
        assert_eq!(f.level_for("wgpu_core"), LevelFilter::Warn);

        let (f, _) = Filter::from_env(Some("debug"));
        assert_eq!((f.level_for("dalica"), f.level_for("naga")), (LevelFilter::Debug, LevelFilter::Debug));

        let (f, _) = Filter::from_env(Some("warn,dalica_scan=trace"));
        assert_eq!(f.level_for("dalica_scan"), LevelFilter::Trace);
        assert_eq!(f.level_for("dalica_scan::walk"), LevelFilter::Trace);
        assert_eq!(f.level_for("dalica_scanner"), LevelFilter::Warn);
        assert_eq!(f.level_for("dalica_fs"), LevelFilter::Warn);
        assert_eq!(f.max(), LevelFilter::Trace);

        let (f, _) = Filter::from_env(Some("info,wgpu_core=warn"));
        assert_eq!((f.level_for("wgpu_core::device"), f.level_for("eframe")), (LevelFilter::Warn, LevelFilter::Info));

        let (f, _) = Filter::from_env(Some("dalica*=debug"));
        assert_eq!(f.level_for("dalica_fs"), LevelFilter::Debug);
        assert_eq!(f.level_for("egui"), LevelFilter::Error);

        // The longest match wins, in either order.
        let (f, _) = Filter::from_env(Some("dalica_fs::atomic=trace,dalica*=warn"));
        assert_eq!((f.level_for("dalica_fs::atomic"), f.level_for("dalica_fs")), (LevelFilter::Trace, LevelFilter::Warn));

        let (f, ignored) = Filter::from_env(Some("info, =debug, x=loud, a b=info, dalica_ui"));
        assert_eq!(ignored, ["=debug", "x=loud", "a b=info"]);
        assert_eq!(f.level_for("dalica_ui"), LevelFilter::Trace);
        assert_eq!(f.level_for("other"), LevelFilter::Info);
    }

    #[test]
    fn config_dir_follows_the_platform_convention() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v));
        if cfg!(windows) {
            assert_eq!(
                config_dir_from(env(&[("APPDATA", r"C:\Users\a\AppData\Roaming")])),
                Some(PathBuf::from(r"C:\Users\a\AppData\Roaming\Dalica"))
            );
            assert_eq!(config_dir_from(env(&[])), None);
        } else if cfg!(target_os = "macos") {
            assert_eq!(config_dir_from(env(&[("HOME", "/Users/a")])), Some(PathBuf::from("/Users/a/Library/Application Support/Dalica")));
        } else {
            assert_eq!(config_dir_from(env(&[("HOME", "/home/a")])), Some(PathBuf::from("/home/a/.config/dalica")));
            assert_eq!(config_dir_from(env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "/x")])), Some(PathBuf::from("/x/dalica")));
            assert_eq!(config_dir_from(env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "relative")])), Some(PathBuf::from("/home/a/.config/dalica")));
            assert_eq!(config_dir_from(env(&[])), None);
        }
    }

    /// Environment pointing the settings and temp folders of a child process at `dir`.
    fn sandbox(cmd: &mut Command, dir: &Path) {
        for var in ["HOME", "XDG_CONFIG_HOME", "APPDATA", "TMPDIR", "TMP", "TEMP"] {
            cmd.env(var, dir);
        }
        cmd.env_remove("RUST_LOG").env("RUST_BACKTRACE", "0");
    }

    const PANIC_HELPER: &str = "DALICA_LOGGING_PANIC_HELPER";

    /// Run as a child process by `a_panic_reaches_both_files`: logs, then panics.
    #[test]
    fn panic_helper() {
        if std::env::var_os(PANIC_HELPER).is_none() {
            return;
        }
        let _guard = init();
        log::info!("about to fail");
        let what = "boom";
        panic!("deliberate {what} 42");
    }

    #[test]
    fn a_panic_reaches_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        // This test's own path inside the binary, minus the crate name: `logging::tests::panic_helper`.
        let helper = format!("{}::panic_helper", module_path!().split_once("::").map_or(module_path!(), |(_, m)| m));
        cmd.args([helper.as_str(), "--exact", "--nocapture", "--test-threads=1"]).env(PANIC_HELPER, "1");
        sandbox(&mut cmd, dir.path());
        let out = cmd.output().unwrap();
        assert!(!out.status.success(), "the helper should have panicked");
        let logs = config_dir_from(|k| if matches!(k, "HOME" | "XDG_CONFIG_HOME" | "APPDATA") { Some(dir.path().into()) } else { None })
            .unwrap()
            .join("logs");
        let log = read(&logs.join(LOG_FILE));
        let panics = read(&dir.path().join(PANICS_FILE));
        for text in [&log, &panics] {
            assert!(text.contains("deliberate boom 42"), "{text}");
            assert!(text.contains("logging.rs:"), "{text}");
            assert!(text.contains("panic in thread `"), "{text}");
        }
        assert!(log.contains("about to fail"), "{log}");
    }

    #[test]
    fn version_help_and_bad_arguments_write_no_file() {
        let status = Command::new(env!("CARGO")).args(["build", "-q", "-p", "dalica"]).status().unwrap();
        assert!(status.success());
        let exe = std::env::current_exe().unwrap();
        let bin = exe.parent().and_then(Path::parent).unwrap().join(format!("dalica{}", std::env::consts::EXE_SUFFIX));
        for args in [&["--version"][..], &["--help"], &["--no-such-flag"]] {
            let dir = tempfile::tempdir().unwrap();
            let mut cmd = Command::new(&bin);
            cmd.args(args);
            sandbox(&mut cmd, dir.path());
            let out = cmd.output().unwrap();
            assert_eq!(out.status.success(), args[0] != "--no-such-flag", "{args:?}");
            let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
            assert!(left.is_empty(), "{args:?} left {left:?}");
        }
    }
}
