//! The applications that open a file on macOS, as Launch Services offers them (`NSWorkspace`'s
//! `URLsForApplicationsToOpenURL:`, the default first through `URLForApplicationToOpenURL:`),
//! through the safe methods of `objc2-app-kit`: no `unsafe` here.

use std::path::{Path, PathBuf};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSURL;

/// The bundles that open `path`, the default first, each once, at most `max`.
pub fn handlers(path: &Path, max: usize) -> Result<Vec<PathBuf>, String> {
    let url = NSURL::from_file_path(path).ok_or_else(|| format!("{} is not a path Launch Services takes", path.display()))?;
    let workspace = NSWorkspace::sharedWorkspace();
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(default) = workspace.URLForApplicationToOpenURL(&url).and_then(|u| u.to_file_path()) {
        out.push(default);
    }
    for app in workspace.URLsForApplicationsToOpenURL(&url).to_vec() {
        if out.len() >= max {
            break;
        }
        if let Some(p) = app.to_file_path() {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    Ok(out)
}
