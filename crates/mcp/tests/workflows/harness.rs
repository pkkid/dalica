//! The workflow harness (`mcp.md` §9): an agent's view of Dalica. A [`Workspace`] is a temporary
//! directory holding `scenes` folders and a library; an [`Agent`] drives a headless MCP server over
//! that library through the real JSON-RPC framing on in-process pipes ([`crate::common::Client`]),
//! calling tools by name as a client would. Workflows check outcomes only through public state
//! (`doc_inspect`, `photo_info`, `query_photos`) and the files on disk: the folder sidecars, the
//! photo files' bytes and their metadata read back by `dalica-meta`, and the stand-in trash.
//!
//! Deletes go to the stand-in trash of `dalica-fs` (its `faults` feature in a debug build: a folder
//! of this process under the system temporary folder), never to the user's trash: [`trashed`]
//! refuses to run in an optimised build, where the real trash would be used.
//!
//! After PhotoCraft `crates/automation/tests/agent_tasks.rs` (`connect`, `tool`, `run`, `inspect`)
//! @ 583425b4, MIT OR Apache-2.0 (`NOTICE`): one session per task, tools called as an agent would,
//! outcomes checked through the public surface.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dalica_mcp::{Headless, Source};
use serde_json::{Value, json};

use crate::common::{Client, initialize};

/// How long [`eventually`] waits for a background write (sidecars and metadata are written after a
/// one-second debounce, on worker threads).
const PATIENCE: Duration = Duration::from_secs(60);

/// The side of the procedural photos, small so a workflow stays quick.
pub const WIDTH: u32 = 160;
pub const HEIGHT: u32 = 120;

/// A temporary directory with photo folders and a library (`library/`), removed at the end.
pub struct Workspace {
    dir: tempfile::TempDir,
}

impl Workspace {
    pub fn new() -> Workspace {
        Workspace { dir: tempfile::tempdir().unwrap() }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// The library directory: kept between two agents of one workspace, so a workflow can quit and
    /// open the library again.
    pub fn library(&self) -> PathBuf {
        self.dir.path().join("library")
    }

    /// A folder `name` of `n` procedural JPEGs (`IMG_0001.jpg` …), scenes `seed`, `seed + 1`, ….
    pub fn folder(&self, name: &str, n: u64, seed: u64) -> PathBuf {
        let folder = self.dir.path().join(name);
        std::fs::create_dir_all(&folder).unwrap();
        for i in 0..n {
            let scene = dalica_scenes::Scene::new(seed + i, WIDTH, HEIGHT);
            std::fs::write(folder.join(photo_name(i + 1)), dalica_scenes::jpeg(&scene, 90, None).unwrap()).unwrap();
        }
        folder
    }

    /// An agent on a headless server over the workspace's library, the workspace a read root (as
    /// `dalica-cli mcp --library DIR --allow-read DIR`).
    pub fn agent(&self) -> Agent {
        let library = self.library();
        let root = self.dir.path().to_path_buf();
        let mut client = Client::start(move || {
            let mut h = Headless::open(Source::Library(library), &[]).unwrap();
            h.roots_mut().add_read(root);
            Box::new(h)
        });
        initialize(&mut client);
        Agent { client, next: 100 }
    }
}

/// The name [`Workspace::folder`] gives its photo number `n` (from 1).
pub fn photo_name(n: u64) -> String {
    format!("IMG_{n:04}.jpg")
}

/// An MCP client calling tools by name, as an agent does.
pub struct Agent {
    client: Client,
    next: u64,
}

impl Agent {
    /// Calls tool `name` and returns the whole result (`isError` included).
    pub fn call(&mut self, name: &str, args: Value) -> Value {
        self.next += 1;
        self.client.call(self.next, name, args)
    }

    /// Calls tool `name` and returns its `structuredContent`, failing on a tool error.
    pub fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.call(name, args.clone());
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }

    /// `scan_folder` of `path`: its section (`folder:<id>`) once scanned.
    pub fn scan(&mut self, path: &Path) -> String {
        let r = self.tool("scan_folder", json!({ "path": path }));
        r["section"].as_str().unwrap_or_else(|| panic!("scan_folder {}: {r}", path.display())).to_string()
    }

    /// The ids of the photos of `section` in light-table order (`doc_inspect`).
    pub fn section_ids(&mut self, section: &str) -> Vec<u64> {
        let r = self.tool("doc_inspect", json!({ "photos": section, "limit": 1000 }));
        ids(&r)
    }

    /// `doc_inspect` without photos.
    pub fn inspect(&mut self) -> Value {
        self.tool("doc_inspect", json!({}))
    }

    /// `photo_info` of `id`.
    pub fn photo(&mut self, id: u64) -> Value {
        self.tool("photo_info", json!({ "id": id }))
    }

    /// The ids `query_photos` answers for `args` (at most 1000).
    pub fn query(&mut self, args: Value) -> Vec<u64> {
        let mut args = args;
        args["limit"] = json!(1000);
        ids(&self.tool("query_photos", args))
    }

    /// Ends the session: the server closes its library, writing what is still pending.
    pub fn quit(self) {
        self.client.close();
    }
}

/// The ids of an answer's `photos` (`doc_inspect`, `query_photos`).
pub fn ids(answer: &Value) -> Vec<u64> {
    answer["photos"].as_array().unwrap_or_else(|| panic!("no photos in {answer}")).iter().map(|p| p["id"].as_u64().unwrap()).collect()
}

/// Polls `check` until it answers `Some`, for [`PATIENCE`]; fails naming `what` and the last
/// state `describe` gives.
pub fn eventually<T>(what: &str, mut check: impl FnMut() -> Option<T>, describe: impl Fn() -> String) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = check() {
            return v;
        }
        assert!(start.elapsed() < PATIENCE, "{what}: still not so after {PATIENCE:?}; now: {}", describe());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The folder's sidecar (`.dalica.json`) as JSON, `Null` when there is none (or it is being
/// replaced at this very moment: it is written atomically, so a read sees the old or the new one).
pub fn sidecar(folder: &Path) -> Value {
    std::fs::read(folder.join(dalica_sidecar::FILE_NAME)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

/// Photo `name`'s entry in the folder's sidecar, `Null` when it has none.
pub fn sidecar_entry(folder: &Path, name: &str) -> Value {
    sidecar(folder).pointer(&format!("/photos/{name}")).cloned().unwrap_or(Value::Null)
}

/// The edits a sidecar entry records (an empty list when none).
pub fn edits(entry: &Value) -> Vec<String> {
    entry["edits"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// The bytes of `path`.
pub fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The files in this process's stand-in trash holding exactly `contents` (the stand-in keeps each
/// item under a number, not its name).
pub fn trashed(contents: &[u8]) -> Vec<PathBuf> {
    if !cfg!(debug_assertions) {
        panic!("the workflows delete into the stand-in trash, which only a debug build uses");
    }
    let root = std::env::temp_dir().join("dalica-test-trash");
    let mine = format!("{}-", std::process::id());
    let mut out = Vec::new();
    for bin in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        if !bin.file_name().to_string_lossy().starts_with(&mine) {
            continue;
        }
        for item in std::fs::read_dir(bin.path().join("files")).into_iter().flatten().flatten() {
            if std::fs::read(item.path()).is_ok_and(|b| b == contents) {
                out.push(item.path());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the workflows delete lands in the stand-in trash, never the user's.
    #[test]
    fn deletes_go_to_the_stand_in_trash() {
        let ws = Workspace::new();
        let path = ws.path().join("doomed.txt");
        let contents = format!("doomed {} {:?}", std::process::id(), std::time::SystemTime::now());
        std::fs::write(&path, &contents).unwrap();
        assert!(matches!(dalica_fs::trash::send(&path).unwrap(), dalica_fs::trash::Sent::Trashed(_)));
        assert!(!path.exists());
        assert_eq!(trashed(contents.as_bytes()).len(), 1, "the file is in this process's stand-in trash");
    }
}
