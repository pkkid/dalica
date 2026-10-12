//! `cargo xtask docs` (a `ci` step): every `cargo xtask …` command named in `AGENTS.md`,
//! `docs/development.md`, `docs/task-board.md` and `docs/contributors.md` exists, either implemented or registered as
//! planned (`crate::PLANNED`), and every `tasks` subcommand they name is one `tasks` knows.

use std::path::Path;

pub const DOCS: &[&str] = &["AGENTS.md", "docs/development.md", "docs/task-board.md", "docs/contributors.md"];

/// `(command, optional tasks subcommand)` for each `xtask <word>` in `text`, with its line.
pub fn mentions(text: &str) -> Vec<(usize, String, Option<String>)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(pos) = rest.find("xtask ") {
            let before_ok = rest.get(..pos).is_none_or(|b| b.is_empty() || b.ends_with([' ', '`', '(', '"', '|']));
            let after = rest.get(pos + 6..).unwrap_or("");
            rest = after;
            if !before_ok {
                continue;
            }
            let word: String = after.chars().take_while(|c| c.is_ascii_lowercase() || *c == '-').collect();
            if word.is_empty() {
                continue;
            }
            let sub = if word == "tasks" {
                let tail = after.get(word.len()..).unwrap_or("");
                let sub: String = tail.strip_prefix(' ').unwrap_or("").chars().take_while(|c| c.is_ascii_lowercase()).collect();
                (!sub.is_empty()).then_some(sub)
            } else {
                None
            };
            out.push((i + 1, word, sub));
        }
    }
    out
}

pub fn check(files: &[(&str, String)], commands: &[&str], planned: &[&str], tasks: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, text) in files {
        for (line, cmd, sub) in mentions(text) {
            if !commands.contains(&cmd.as_str()) && !planned.contains(&cmd.as_str()) {
                out.push(format!("{name}:{line}: `cargo xtask {cmd}` does not exist (implement it, register it as planned, or fix the doc)"));
            }
            if let Some(sub) = sub
                && !tasks.contains(&sub.as_str())
            {
                out.push(format!("{name}:{line}: `cargo xtask tasks {sub}` is not a tasks command"));
            }
        }
    }
    out
}

pub fn run(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    for name in DOCS {
        let text = std::fs::read_to_string(root.join(name)).map_err(|e| format!("cannot read {name}: {e}"))?;
        files.push((*name, text));
    }
    let planned: Vec<&str> = crate::PLANNED.iter().map(|(c, _)| *c).collect();
    let problems = check(&files, crate::COMMANDS, &planned, crate::tasks::SUBCOMMANDS);
    for p in &problems {
        eprintln!("docs: {p}");
    }
    if problems.is_empty() {
        let n: usize = files.iter().map(|(_, t)| mentions(t).len()).sum();
        println!("docs: ok ({n} `cargo xtask` mentions in {} files)", files.len());
        Ok(())
    } else {
        Err(format!("docs: {} problem(s)", problems.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_commands_and_tasks_subcommands() {
        let m = mentions("run `cargo xtask ci` then cargo xtask tasks claim M1.4; | `xtask parity [--write]` |\nmy_xtask thing");
        let got: Vec<_> = m.iter().map(|(l, c, s)| (*l, c.as_str(), s.as_deref())).collect();
        assert_eq!(got, [(1, "ci", None), (1, "tasks", Some("claim")), (1, "parity", None)]);
    }

    #[test]
    fn unknown_commands_and_subcommands_fail_planned_ones_pass() {
        let files = [("AGENTS.md", "cargo xtask ci\ncargo xtask bench\ncargo xtask frobnicate\ncargo xtask tasks fly".to_string())];
        let p = check(&files, &["ci", "tasks"], &["bench"], &["claim"]);
        assert_eq!(
            p,
            [
                "AGENTS.md:3: `cargo xtask frobnicate` does not exist (implement it, register it as planned, or fix the doc)",
                "AGENTS.md:4: `cargo xtask tasks fly` is not a tasks command"
            ]
        );
    }

    #[test]
    fn the_corpus_commands_are_implemented_not_planned() {
        for cmd in ["corpus", "test-corpus"] {
            assert!(crate::COMMANDS.contains(&cmd), "{cmd}");
            assert!(!crate::PLANNED.iter().any(|(c, _)| *c == cmd), "{cmd} is still listed as planned");
        }
        // development.md §11 names both.
        let text = std::fs::read_to_string(crate::workspace_root().join("docs/development.md")).unwrap();
        let named: Vec<String> = mentions(&text).into_iter().map(|(_, c, _)| c).collect();
        assert!(named.iter().any(|c| c == "corpus") && named.iter().any(|c| c == "test-corpus"), "{named:?}");
    }

    #[test]
    fn version_is_implemented_and_documented() {
        assert!(crate::COMMANDS.contains(&"version"));
        assert!(!crate::PLANNED.iter().any(|(c, _)| *c == "version"), "version is still listed as planned");
        // development.md §3 lists it.
        let text = std::fs::read_to_string(crate::workspace_root().join("docs/development.md")).unwrap();
        assert!(mentions(&text).iter().any(|(_, c, _)| c == "version"));
    }

    #[test]
    fn contributors_is_implemented_and_documented() {
        assert!(crate::COMMANDS.contains(&"contributors"));
        assert!(!crate::PLANNED.iter().any(|(c, _)| *c == "contributors"), "contributors is still listed as planned");
        // development.md §3 and contributors.md → Refreshing name it, both checked by this command.
        for doc in ["docs/development.md", "docs/contributors.md"] {
            assert!(DOCS.contains(&doc));
            let text = std::fs::read_to_string(crate::workspace_root().join(doc)).unwrap();
            assert!(mentions(&text).iter().any(|(_, c, _)| c == "contributors"), "{doc}");
        }
    }

    #[test]
    fn the_real_docs_pass() {
        let root = crate::workspace_root();
        let files: Vec<(&str, String)> = DOCS.iter().map(|n| (*n, std::fs::read_to_string(root.join(n)).unwrap())).collect();
        let planned: Vec<&str> = crate::PLANNED.iter().map(|(c, _)| *c).collect();
        assert_eq!(check(&files, crate::COMMANDS, &planned, crate::tasks::SUBCOMMANDS), Vec::<String>::new());
    }
}
