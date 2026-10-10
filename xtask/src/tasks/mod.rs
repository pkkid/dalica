//! `cargo xtask tasks …`: the task board (`docs/task-board.md`).

pub mod board;
mod body;
pub mod config;
mod devlog;
mod dryrun;
#[cfg(test)]
mod fake;
mod gh;
mod git;
mod github;
mod glob;
mod lock;
pub mod roadmap;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use board::Board;
use git::{Clock, Git, RealGit, SystemClock};
use lock::Lock;

const USAGE: &str = "usage: cargo xtask tasks <command>

  setup [--dry-run]              create milestones, labels and project fields (idempotent)
  sync [--check|--dry-run]       make the issues and the project match roadmap.md (--check: parse only, offline;
                                 --dry-run: read GitHub, print the writes it would make, make none)
  next [--agent NAME]            tasks you may claim, in roadmap order (claim the first)
  claim ID --agent NAME          claim a Ready task; prints the worktree to work in
  env NAME                       the agent's environment: eval \"$(cargo xtask tasks env agent-3)\"
  submit ID [--body-file FILE]   push, open the pull request, set In Review (needs a green ci and a dev-log entry)
  release ID --reason TEXT       give a claimed task back (the branch is kept)
  block ID \"QUESTION\"            ask the maintainer; the task becomes Blocked
  merge ID                       lead only: rebase, ci, push, wait for GitHub CI, squash-merge, sync
  status                         the whole board; marks idle claims stale
  log ID \"TEXT\" [--agent NAME]   append to the private dev log (log/devlog.md in the main checkout)

NAME defaults to $DALICA_AGENT.";

/// Every `cargo xtask tasks` subcommand (`cargo xtask docs` checks the docs against it).
pub const SUBCOMMANDS: &[&str] = &["setup", "sync", "next", "claim", "env", "submit", "release", "block", "merge", "status", "log"];

/// Positional arguments and `--flag value` pairs.
struct Args {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(args: &[String]) -> Args {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut it = args.iter().peekable();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                let value = match it.peek() {
                    Some(v) if !v.starts_with("--") && !matches!(name, "check" | "dry-run") => it.next().cloned(),
                    _ => None,
                };
                flags.push((name.to_string(), value));
            } else {
                positional.push(a.clone());
            }
        }
        Args { positional, flags }
    }

    fn flag(&self, name: &str) -> Option<&str> {
        self.flags.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.as_deref())
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }

    fn pos(&self, i: usize, what: &str) -> Result<&str, String> {
        self.positional.get(i).map(String::as_str).ok_or_else(|| format!("missing {what}\n\n{USAGE}"))
    }

    fn agent(&self) -> Result<String, String> {
        self.flag("agent")
            .map(str::to_string)
            .or_else(|| std::env::var("DALICA_AGENT").ok().filter(|a| !a.is_empty()))
            .ok_or_else(|| "say which agent you are: --agent agent-<n> (or set DALICA_AGENT)".into())
    }
}

pub fn load_roadmap(root: &Path) -> Result<roadmap::Roadmap, String> {
    let path = root.join("docs").join("roadmap.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    roadmap::parse(&text).map_err(|e| e.to_string())
}

/// `agent-3` → 3.
fn agent_number(agent: &str) -> Result<u32, String> {
    agent
        .strip_prefix("agent-")
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (1..=99).contains(n))
        .ok_or_else(|| format!("agent names look like agent-<1..99>, not `{agent}`"))
}

/// The per-agent environment of `docs/task-board.md` §7, as shell `export` lines.
pub fn env_lines(agent: &str, main: &Path, scratch_base: &Path) -> Result<String, String> {
    let n = agent_number(agent)?;
    let repo = main.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dalica".into());
    let worktree = main.parent().unwrap_or(main).join(format!("{repo}.worktrees")).join(agent);
    let q = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
    let mut out = String::new();
    out.push_str(&format!("export DALICA_AGENT={agent}\n"));
    out.push_str(&format!("export DALICA_WORKTREE={}\n", q(&worktree)));
    out.push_str(&format!("export CARGO_TARGET_DIR={}\n", q(&main.join("target").join(agent))));
    out.push_str("export CARGO_BUILD_JOBS=3\n");
    out.push_str(&format!("export DALICA_CONTROL_PORT={}\n", 18000 + 10 * n));
    out.push_str(&format!("export DALICA_SCRATCH={}\n", q(&scratch_base.join(agent))));
    if worktree.is_dir() {
        out.push_str(&format!("cd {}\n", q(&worktree)));
    }
    Ok(out)
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let Some((cmd, rest)) = args.split_first() else {
        println!("{USAGE}");
        return Ok(());
    };
    let a = Args::parse(rest);
    let roadmap = load_roadmap(root)?;
    if cmd == "sync" && a.has("check") {
        println!("tasks: roadmap.md parses: {} milestones, {} task blocks", roadmap.milestones.len(), roadmap.tasks.len());
        return Ok(());
    }
    let git = RealGit { cwd: std::env::current_dir().unwrap_or_else(|_| root.to_path_buf()) };
    let real = gh::GhCli::default();
    let dry = dryrun::DryRun::new(&real);
    let dry_run = a.has("dry-run");
    if dry_run && !matches!(cmd.as_str(), "setup" | "sync") {
        return Err("--dry-run works with setup and sync".into());
    }
    let gh: &dyn github::GitHub = if dry_run { &dry } else { &real };
    let clock = SystemClock;
    let board = Board { roadmap: &roadmap, gh, git: &git, clock: &clock };
    let claim_lock = || Lock::acquire(&git.common_dir().join("dalica-tasks.lock"));
    match cmd.as_str() {
        "setup" => {
            let changes = board.setup()?;
            print_changes("setup", &changes);
        }
        "sync" => {
            let _lock = claim_lock()?;
            let report = board.sync()?;
            print_changes("sync", &report.changes);
        }
        "next" => {
            let tasks = board.next()?;
            if tasks.is_empty() {
                println!("nothing to claim right now; ask again in 5 minutes");
            }
            for t in tasks {
                println!("{}  {}", t.id, t.plain_title());
            }
        }
        "claim" => {
            let id = a.pos(0, "task id")?;
            let agent = a.agent()?;
            agent_number(&agent)?;
            let claim = {
                let _lock = claim_lock()?;
                board.claim(id, &agent)?
            };
            println!("claimed {} on {}; work in {}", claim.id, claim.branch, claim.worktree.display());
            print!("{}", env_lines(&agent, &git.main_checkout(), &scratch_base())?);
        }
        "env" => {
            let agent = a.positional.first().cloned().map_or_else(|| a.agent(), Ok)?;
            print!("{}", env_lines(&agent, &git.main_checkout(), &scratch_base())?);
        }
        "submit" => {
            let id = a.pos(0, "task id")?;
            let extra = match a.flag("body-file") {
                Some(f) => Some(std::fs::read_to_string(f).map_err(|e| format!("cannot read {f}: {e}"))?),
                None => None,
            };
            let dir = std::env::current_dir().map_err(|e| e.to_string())?;
            let url = {
                let _lock = claim_lock()?;
                board.submit(id, &dir, extra.as_deref())?
            };
            println!("submitted {id}: {url}");
        }
        "release" => {
            let id = a.pos(0, "task id")?;
            let reason = a.flag("reason").ok_or("release needs --reason \"…\"")?;
            let by = a.agent().unwrap_or_else(|_| "someone".into());
            let _lock = claim_lock()?;
            board.release(id, &by, reason)?;
            println!("released {id}");
        }
        "block" => {
            let id = a.pos(0, "task id")?;
            let question = a.pos(1, "question")?;
            let by = a.agent().unwrap_or_else(|_| "someone".into());
            let _lock = claim_lock()?;
            board.block(id, &by, question)?;
            println!("{id} is Blocked; take another task");
        }
        "merge" => {
            let id = a.pos(0, "task id")?;
            if a.agent()? != config::LEAD {
                return Err(format!("only the lead ({}) merges", config::LEAD));
            }
            let _merge = Lock::acquire(&git.common_dir().join("dalica-merge.lock"))?;
            let subject = board.merge(id)?;
            println!("merged: {subject}");
            let _lock = claim_lock()?;
            print_changes("sync", &board.sync()?.changes);
        }
        "status" => print!("{}", board.status()?),
        "log" => {
            let id = a.pos(0, "task id")?;
            let text = a.pos(1, "text")?;
            let agent = a.agent()?;
            let task = roadmap.task(id).ok_or_else(|| format!("{id} is not a task in docs/roadmap.md"))?;
            let entry = devlog::entry(clock.now(), clock.utc_offset(), &agent, &format!("{} {}", task.id, task.plain_title()), text);
            let path = devlog::path(&git.main_checkout());
            let _lock = claim_lock()?;
            devlog::append(&path, &entry)?;
            println!("appended to {}", path.display());
        }
        other => return Err(format!("unknown tasks command `{other}`\n\n{USAGE}")),
    }
    if dry_run {
        let writes = dry.writes();
        println!("dry run: {} write(s) would be made, none were:", writes.len());
        for w in writes {
            println!("  {w}");
        }
    }
    Ok(())
}

fn scratch_base() -> PathBuf {
    std::env::var_os("DALICA_SCRATCH_BASE").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("dalica-scratch"))
}

fn print_changes(what: &str, changes: &[String]) {
    if changes.is_empty() {
        println!("{what}: nothing to change");
    }
    for c in changes {
        println!("{what}: {c}");
    }
}
