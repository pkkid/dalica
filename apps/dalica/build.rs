//! Windows only: embed the app icon and the version resource (VERSIONINFO) into `dalica.exe`, and
//! link release builds as a GUI program, so no console window opens beside Dalica
//! (`docs/releasing.md` → Windows). On every other target this does nothing.
//!
//! Adapted from PhotoCraft `apps/photocraft/build.rs` (revision 1f1ddb2a) and
//! `apps/photocraft-cli/build.rs` (revision b885173d); MIT OR Apache-2.0. Changed for Dalica: a resource compiler that is missing
//! or fails is always an error naming the tool (never a warning, never a panic), the resource is
//! linked into the `dalica` binary only, and the GUI subsystem is chosen here through linker
//! arguments for release builds, so debug builds and tests keep their console.
//!
//! The resource compiler is the Windows SDK's `rc.exe` (found through the registry by
//! `winresource`), `llvm-rc` when cross-compiling from Linux or macOS to an MSVC target, or MinGW's
//! `windres` for a GNU target; `RC_PATH` (MSVC) or `WINDRES` (GNU) names it explicitly.
//! `packaging/windows/test-build-rs.ps1` checks both outcomes on Windows.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::env;
use std::fmt;
use std::io;
use std::path::PathBuf;

/// The icon, relative to this crate's manifest folder.
const ICON: &str = "../../assets/app-icon/dalica.ico";
/// The binary that gets the resource and the GUI subsystem (`dalica-cli` stays a console program).
const BIN: &str = "dalica";

/// A build failure, printed as plain text (`main` returning `Err` prints its `Debug`).
struct Failure(String);

impl fmt::Debug for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn main() -> Result<(), Failure> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    for var in ["RC_PATH", "WINDRES", "CROSS_COMPILE"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    embed_resources()?;
    if env::var("PROFILE").as_deref() == Ok("release") {
        gui_subsystem();
    }
    Ok(())
}

/// Compiles the icon and VERSIONINFO and links them into `dalica.exe`. The version fields come
/// from the package version (`[workspace.package] version`): the strings keep a pre-release tag
/// (`0.2.0-rc.1`), the numeric FILEVERSION and PRODUCTVERSION are `0.2.0.0`.
fn embed_resources() -> Result<(), Failure> {
    let manifest = env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).ok_or_else(|| Failure("CARGO_MANIFEST_DIR is not set".into()))?;
    let icon = manifest.join(ICON);
    if !icon.is_file() {
        return Err(Failure(format!("the app icon {} is missing (assets/app-icon, packaging/icons.sh)", icon.display())));
    }
    let icon = icon.to_str().ok_or_else(|| Failure(format!("the app icon path {} is not valid UTF-8", icon.display())))?;
    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon)
        .set("ProductName", "Dalica")
        // Windows shows the description as the program's name (Task Manager, Open with).
        .set("FileDescription", "Dalica")
        .set("CompanyName", "the Dalica contributors")
        .set("LegalCopyright", "Copyright (c) the Dalica contributors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "dalica.exe")
        .set("InternalName", "dalica");
    res.compile_for(BIN).map_err(|e| explain(&e))
}

/// The resource compiler this build uses, named the way the error should name it.
fn tool() -> String {
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let set = |var: &str| env::var_os(var).filter(|v| !v.is_empty()).map(|v| PathBuf::from(v).display().to_string());
    if msvc {
        // `cfg!(windows)` is the host here: a build script runs where it is built.
        set("RC_PATH").unwrap_or_else(|| if cfg!(windows) { "rc.exe (Windows SDK)".into() } else { "llvm-rc".into() })
    } else {
        set("WINDRES").unwrap_or_else(|| "windres (MinGW-w64)".into())
    }
}

/// The error for a failed resource step: which tool, what happened, how to fix it.
fn explain(e: &io::Error) -> Failure {
    let tool = tool();
    let fix = "Install the Windows SDK (Visual Studio Build Tools include it) for rc.exe, or name the compiler with RC_PATH (MSVC targets) or WINDRES (GNU targets).";
    let what = if e.kind() == io::ErrorKind::NotFound {
        format!("the resource compiler {tool} was not found ({e})")
    } else {
        format!("the resource compiler {tool} failed: {e} (its output is above)")
    };
    Failure(format!("dalica.exe needs its icon and version resource, but {what}. {fix}"))
}

/// Release builds of `dalica.exe` are GUI programs: no console window opens with them. The entry
/// point stays the C runtime's `main` startup, so `fn main` runs as before.
fn gui_subsystem() {
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin={BIN}=/SUBSYSTEM:WINDOWS");
        println!("cargo:rustc-link-arg-bin={BIN}=/ENTRY:mainCRTStartup");
    } else {
        println!("cargo:rustc-link-arg-bin={BIN}=-Wl,--subsystem,windows");
    }
}
