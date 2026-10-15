//! `dalica --version`, `--help` and a bad argument answer and exit without writing a file: no
//! log, no settings, nothing in the temp folder.

use crate::common::sandbox;
use std::process::Command;

#[test]
fn version_help_and_bad_arguments_write_no_file() {
    for args in [&["--version"][..], &["--help"], &["--no-such-flag"]] {
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dalica"));
        cmd.args(args);
        sandbox(&mut cmd, dir.path());
        cmd.env("RUST_BACKTRACE", "0");
        let out = cmd.output().unwrap();
        assert_eq!(out.status.success(), args[0] != "--no-such-flag", "{args:?}");
        // The sandbox's temp folder is made for the run; nothing else may appear.
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n != "tmp").collect();
        assert!(left.is_empty(), "{args:?} left {left:?}");
        assert_eq!(std::fs::read_dir(dir.path().join("tmp")).unwrap().count(), 0, "{args:?} wrote into the temp folder");
    }
}
