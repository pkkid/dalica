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
mod prune;
#[cfg(test)]
mod repo_tests;
pub mod roadmap;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use board::Board;
use git::{Clock, Git, RealGit, SystemClock};
use github::GitHub;
use lock::Lock;

const USAGE: &str = "usage: cargo xtask tasks <command>

  setup [--dry-run]              create labels and project fields; create, rename and describe milestones to match
                                 roadmap.md, close empty ones it no longer lists (idempotent)
  sync [--check|--dry-run]       make the issues and the project match roadmap.md (--check: parse only, offline;
                                 --dry-run: read GitHub, print the writes it would make, make none)
  next [--agent NAME]            tasks you may claim, in roadmap order (claim the first); nothing listed after an
                                 open review task (M<n>.R) until the maintainer closes it
  claim ID --agent NAME          claim a Ready task; prints the worktree to work in
  env NAME                       the agent's environment: eval \"$(cargo xtask tasks env agent-3)\"
  submit ID [--body-file FILE] [--dry-run]
                                 push, open the pull request, set In Review (needs a green ci and a dev-log entry);
                                 without --body-file the evidence is the task's newest dev-log entry; never a review task;
                                 an In Review task sent back by merge is submitted again once rebased (pushed with
                                 lease, the same pull request)
  release ID --reason TEXT       give a claimed task back (the branch is kept)
  block ID \"QUESTION\"            ask the maintainer; the task becomes Blocked
  merge ID [--dry-run]           lead only: refuse while main is red on GitHub CI, rebase (a conflict only in
                                 docs/parity.md's summary is resolved by regenerating it; any other conflict sends
                                 the task back to its worker), ci, push, wait until
                                 GitHub finds the PR mergeable, squash-merge, record added, changed or deleted
                                 fingerprints in the dev log (FINGERPRINTS: lines, approved in bulk at the next
                                 review), sync against origin/main's roadmap.md; never a review task (only the
                                 maintainer merges or closes those)
  status                         the whole board; marks idle claims stale
  log ID \"TEXT\" [--agent NAME]   append to the private dev log (log/devlog.md in the main checkout)
  prune [--dry-run]              lead only: remove incremental/ from the target dirs of agents with no In Progress task

--dry-run reads GitHub and git, prints the writes it would make (and what prune would remove), makes none.

NAME defaults to $DALICA_AGENT.";

/// Every `cargo xtask tasks` subcommand (`cargo xtask docs` checks the docs against it).
pub const SUBCOMMANDS: &[&str] = &["setup", "sync", "next", "claim", "env", "submit", "release", "block", "merge", "status", "log", "prune"];

/// The commands `--dry-run` works with.
const DRY_RUNNABLE: &[&str] = &["setup", "sync", "submit", "merge", "prune"];

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
    let real_git = RealGit { cwd: std::env::current_dir().unwrap_or_else(|_| root.to_path_buf()) };
    let real = gh::GhCli::default();
    let recorder = dryrun::Recorder::default();
    let dry = dryrun::DryRun::new(&real, &recorder);
    let dry_git = dryrun::DryGit::new(&real_git, &recorder);
    let dry_run = a.has("dry-run");
    if dry_run && !DRY_RUNNABLE.contains(&cmd.as_str()) {
        return Err(format!("--dry-run works with {}", DRY_RUNNABLE.join(", ")));
    }
    let gh: &dyn GitHub = if dry_run { &dry } else { &real };
    let git: &dyn Git = if dry_run { &dry_git } else { &real_git };
    let clock = SystemClock;
    let board = Board { roadmap: &roadmap, gh, git, clock: &clock };
    // A dry run writes nothing, so it takes no lock (and never waits for the lead's merge).
    let lock = |name: &str| if dry_run { Ok(None) } else { Lock::acquire(&git.common_dir().join(name)).map(Some) };
    let claim_lock = || lock("dalica-tasks.lock");
    match cmd.as_str() {
        "setup" => {
            let report = board.setup()?;
            print_changes("setup", &report.changes);
            for note in &report.notes {
                println!("setup: note: {note}");
            }
        }
        "sync" => {
            let _lock = claim_lock()?;
            print_sync(&board.sync()?);
        }
        "next" => {
            let agent = a.agent().ok();
            let tasks = board.next(agent.as_deref())?;
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
            let _merge = lock("dalica-merge.lock")?;
            let subject = board.merge(id)?;
            if dry_run {
                // Nothing was merged, so the sync that follows a merge would only see the branch's
                // ✅ without its commit on main and refuse.
                println!("would merge: {subject}, then sync");
            } else {
                println!("merged: {subject}");
                // The main checkout's roadmap.md predates the merge (it lacks the task's ✅), so the
                // sync reads the roadmap as merged on origin/main; no checkout is touched.
                let _lock = claim_lock()?;
                print_sync(&board.sync_merged()?);
            }
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
        "prune" => {
            if a.agent()? != config::LEAD {
                return Err(format!("only the lead ({}) prunes", config::LEAD));
            }
            let report = board.prune(&git.main_checkout().join("target"), dry_run)?;
            for line in &report.lines {
                println!("prune: {line}");
            }
            println!("prune: {} {}", if dry_run { "would free" } else { "freed" }, prune::human(report.freed));
            if report.errors > 0 {
                return Err(format!("prune: {} removal(s) failed", report.errors));
            }
        }
        other => return Err(format!("unknown tasks command `{other}`\n\n{USAGE}")),
    }
    if dry_run {
        let writes = recorder.writes();
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

fn print_sync(report: &board::SyncReport) {
    print_changes("sync", &report.changes);
    for note in &report.notes {
        println!("sync: note: {note}");
    }
}

fn print_changes(what: &str, changes: &[String]) {
    if changes.is_empty() {
        println!("{what}: nothing to change");
    }
    for c in changes {
        println!("{what}: {c}");
    }
}
