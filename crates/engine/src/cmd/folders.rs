//! Watched folders. `folders.add` is what File → Add Folder… runs after the folder picker.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "folders.add",
        label: "Add Folder…",
        menu: "File/Add Folder…",
        keys: &[],
        params: r#"{"path":string (absolute)}: watch a folder and scan it in the background"#,
        effect: Effect::Library,
        history: History::None,
        enabled: always,
        run: add,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Add {
    path: PathBuf,
}

fn add(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Add { path } = params(p)?;
    if !path.is_absolute() || path.as_os_str().len() > 4096 {
        return Err(CmdError::BadParams(format!("{} is not an absolute folder path", path.display())));
    }
    let new = s.add_root(path.clone());
    Ok(json!({ "path": path, "new": new, "scanning": true }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::{hostile, library, open};
    use crate::error::CmdError;

    #[test]
    fn add_scans_in_the_background_and_persists_the_folder() {
        let (dirs, mut s, ids) = library(3);
        assert_eq!(ids.len(), 3);
        assert_eq!(s.roots().len(), 1);
        // Adding the same folder again rescans and finds nothing new.
        let again = s.execute("folders.add", &json!({ "path": s.roots()[0] })).unwrap();
        assert_eq!(again["new"], false);
        s.wait_idle();
        assert_eq!(s.library().photos().count(), 3);
        assert!(matches!(s.execute("folders.add", &json!({ "path": "relative/dir" })), Err(CmdError::BadParams(_))));
        hostile(&mut s, "folders.add");
        s.flush().unwrap();
        let lib_dir = s.dir().to_path_buf();
        drop(s);
        // A new session remembers the folder and the photos.
        let s = open(&lib_dir);
        assert_eq!(s.roots().len(), 1);
        assert_eq!(s.library().photos().count(), 3);
        drop(dirs);
    }
}
