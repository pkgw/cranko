// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! The main cranko command-line interface.
//!
//! This just provides swiss-army-knife access to commands installed by other
//! Cranko modules. Not 100% sure this is the way to got but we'll see.
//!
//! Heavily modeled on Cargo's implementation of the same sort of functionality.

use anyhow::{anyhow, Context, Result};
use log::{error, info, warn};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};
use structopt::StructOpt;

mod app;
mod changelog;
mod errors;
mod github;
mod graph;
mod loaders;
mod logger;
mod project;
mod repository;
mod rewriters;
mod version;

#[derive(Debug, PartialEq, StructOpt)]
#[structopt(about = "automate versioning and releasing")]
struct CrankoOptions {
    #[structopt(subcommand)]
    command: Commands,
}

trait Command {
    fn execute(self) -> Result<i32>;
}

#[derive(Debug, PartialEq, StructOpt)]
enum Commands {
    #[structopt(name = "confirm")]
    /// Commit staged release requests to the `rc` branch
    Confirm(ConfirmCommand),

    #[structopt(name = "github")]
    /// GitHub release utilities
    Github(github::GithubCommand),

    #[structopt(name = "help")]
    /// Prints this message or the help of the given subcommand
    Help(HelpCommand),

    #[structopt(name = "list-commands")]
    /// List available subcommands
    ListCommands(ListCommandsCommand),

    #[structopt(name = "release-workflow")]
    /// Specialized operations for releases in the just-in-time versioning workflow
    ReleaseWorkflow(ReleaseWorkflowCommand),

    #[structopt(name = "show")]
    /// Print out various useful pieces of information
    Show(ShowCommand),

    #[structopt(name = "stage")]
    /// Mark one or more projects as planned for release
    Stage(StageCommand),

    #[structopt(name = "status")]
    /// Report release status inside the active repo
    Status(StatusCommand),

    #[structopt(external_subcommand)]
    External(Vec<String>),
}

impl Command for Commands {
    fn execute(self) -> Result<i32> {
        match self {
            Commands::Confirm(o) => o.execute(),
            Commands::Github(o) => o.execute(),
            Commands::Help(o) => o.execute(),
            Commands::ListCommands(o) => o.execute(),
            Commands::ReleaseWorkflow(o) => o.execute(),
            Commands::Show(o) => o.execute(),
            Commands::Stage(o) => o.execute(),
            Commands::Status(o) => o.execute(),
            Commands::External(args) => do_external(args),
        }
    }
}

fn main() -> Result<()> {
    let opts = CrankoOptions::from_args();

    if let Err(e) = logger::Logger::init() {
        eprintln!("error: cannot initialize logging backend: {}", e);
        std::process::exit(1);
    }
    log::set_max_level(log::LevelFilter::Info);

    let exitcode = match opts.command.execute() {
        Ok(c) => c,
        Err(e) => {
            error!("{}", e);
            e.chain()
                .skip(1)
                .for_each(|cause| logger::Logger::print_cause(cause));
            1
        }
    };

    std::process::exit(exitcode);
}

// confirm

#[derive(Debug, PartialEq, StructOpt)]
struct ConfirmCommand {}

impl Command for ConfirmCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        sess.populated_graph()?;

        let mut changes = repository::ChangeList::default();
        let mut rc_info = Vec::new();

        for proj in sess.graph().toposort()? {
            if let Some(info) = sess.repo.scan_rc_info(proj, &mut changes)? {
                // TODO: apply bump and print out expected new version and other helpful info
                info!("{}: {}", proj.user_facing_name, info.bump_spec);
                rc_info.push(info);
            }
        }

        if rc_info.len() < 1 {
            warn!("no releases seem to have been staged; use \"cranko stage\"?");
            return Ok(0);
        }

        sess.make_rc_commit(rc_info, &changes)?;
        info!(
            "staged rc commit to `{}` branch",
            sess.repo.upstream_rc_name()
        );
        Ok(0)
    }
}

// help

#[derive(Debug, PartialEq, StructOpt)]
struct HelpCommand {
    command: Option<String>,
}

impl Command for HelpCommand {
    fn execute(self) -> Result<i32> {
        match self.command.as_deref() {
            None => {
                CrankoOptions::clap().print_long_help()?;
                println!();
                Ok(0)
            }

            Some(cmd) => {
                CrankoOptions::from_iter(&[&std::env::args().next().unwrap(), cmd, "--help"])
                    .command
                    .execute()
            }
        }
    }
}

// list-commands

#[derive(Debug, PartialEq, StructOpt)]
struct ListCommandsCommand {}

impl Command for ListCommandsCommand {
    fn execute(self) -> Result<i32> {
        println!("Currently available \"cranko\" subcommands:\n");

        for command in list_commands() {
            println!("    {}", command);
        }

        Ok(0)
    }
}

// release-workflow

#[derive(Debug, PartialEq, StructOpt)]
struct ReleaseWorkflowCommand {
    #[structopt(subcommand)]
    command: ReleaseWorkflowCommands,
}

#[derive(Debug, PartialEq, StructOpt)]
enum ReleaseWorkflowCommands {
    #[structopt(name = "apply-versions")]
    /// Apply version numbers to all projects in the working tree.
    ApplyVersions(ReleaseWorkflowApplyVersionsCommand),

    #[structopt(name = "commit")]
    /// Commit changes as a new release
    Commit(ReleaseWorkflowCommitCommand),

    #[structopt(name = "tag")]
    /// Create version-control tags for new releases
    Tag(ReleaseWorkflowTagCommand),
}

impl Command for ReleaseWorkflowCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            ReleaseWorkflowCommands::ApplyVersions(o) => o.execute(),
            ReleaseWorkflowCommands::Commit(o) => o.execute(),
            ReleaseWorkflowCommands::Tag(o) => o.execute(),
        }
    }
}

// release-workflow apply-versions

#[derive(Debug, PartialEq, StructOpt)]
struct ReleaseWorkflowApplyVersionsCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force operation even in unexpected conditions"
    )]
    force: bool,
}

impl Command for ReleaseWorkflowApplyVersionsCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        sess.populated_graph()?;

        sess.repo.check_dirty()?;

        let mut rci = None;
        let mut dev_mode = true;

        match sess.execution_environment() {
            app::ExecutionEnvironment::NotCi => {
                warn!("no CI environment detected; this is unexpected for this command");
                if !self.force {
                    return Err(anyhow!("refusing to proceed (use `--force` to override)"));
                }
            }

            app::ExecutionEnvironment::CiDevelopmentBranch
            | app::ExecutionEnvironment::CiPullRequest => {
                info!("detected development-mode CI environment");
            }

            app::ExecutionEnvironment::CiRcBranch => {
                info!("computing new versions based on `rc` commit request data");
                rci = Some(sess.repo.parse_rc_info_from_head()?);
                dev_mode = false;
            }

            app::ExecutionEnvironment::CiReleaseBranch => {
                warn!(
                    "detected CI environment on `release` branch; this is unexpected for this command"
                );
                if !self.force {
                    return Err(anyhow!("refusing to proceed (use `--force` to override)"));
                }
            }
        }

        let rci = rci.unwrap_or_else(|| sess.default_dev_rc_info());

        sess.apply_versions(&rci)?;
        let mut changes = sess.rewrite()?;

        if !dev_mode {
            sess.apply_changelogs(&rci, &mut changes)?;
        }

        Ok(0)
    }
}

// release-workflow commit

#[derive(Debug, PartialEq, StructOpt)]
struct ReleaseWorkflowCommitCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force operation even in unexpected conditions"
    )]
    force: bool,
}

impl Command for ReleaseWorkflowCommitCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        sess.populated_graph()?;

        match sess.execution_environment() {
            app::ExecutionEnvironment::NotCi => {
                warn!("no CI environment detected; this is unexpected for this command");
                if !self.force {
                    return Err(anyhow!("refusing to proceed (use `--force` to override)"));
                }
            }

            app::ExecutionEnvironment::CiDevelopmentBranch
            | app::ExecutionEnvironment::CiPullRequest
            | app::ExecutionEnvironment::CiReleaseBranch => {
                warn!("CI environment detected but not on `rc` branch; this is unexpected for this command");
                if !self.force {
                    return Err(anyhow!("refusing to proceed (use `--force` to override)"));
                }
            }

            app::ExecutionEnvironment::CiRcBranch => {}
        }

        // We don't actually need the information, but this step helps us verify
        // that everything is as expected.
        sess.repo
            .parse_rc_info_from_head()
            .context("expected HEAD to contain `rc` information")?;

        sess.make_release_commit()?;
        Ok(0)
    }
}

// release-workflow tag

#[derive(Debug, PartialEq, StructOpt)]
struct ReleaseWorkflowTagCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force operation even in unexpected conditions"
    )]
    force: bool,
}

impl Command for ReleaseWorkflowTagCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;

        // Note here that the active checkout should have been switched to the
        // `release` branch by `release-workflow commit` or an equivalent; but
        // the CI run should still have been *triggered* for the `rc` branch.
        match sess.execution_environment() {
            app::ExecutionEnvironment::NotCi => {
                warn!("no CI environment detected; this is unexpected for this command");
                if !self.force {
                    return Err(anyhow!("refusing to proceed (use `--force` to override)"));
                }
            }

            app::ExecutionEnvironment::CiDevelopmentBranch
            | app::ExecutionEnvironment::CiPullRequest
            | app::ExecutionEnvironment::CiReleaseBranch => {
                warn!("CI environment detected but not on `rc` branch; this is unexpected for this command");
                if !self.force {
                    return Err(anyhow!("refusing to proceed (use `--force` to override)"));
                }
            }

            app::ExecutionEnvironment::CiRcBranch => {}
        }

        let rci = sess.repo.parse_release_info_from_head()?;
        sess.create_tags(&rci)?;
        Ok(0)
    }
}

// show

#[derive(Debug, PartialEq, StructOpt)]
struct ShowCommand {
    #[structopt(subcommand)]
    command: ShowCommands,
}

#[derive(Debug, PartialEq, StructOpt)]
enum ShowCommands {
    #[structopt(name = "version")]
    /// Print the current version number of a project
    Version(ShowVersionCommand),
}

impl Command for ShowCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            ShowCommands::Version(o) => o.execute(),
        }
    }
}

#[derive(Debug, PartialEq, StructOpt)]
struct ShowVersionCommand {
    // TODO: add something like `--ifdev=latest` to print "latest"
    // instead of 0.0.0-dev.0 if we're not on a release commit for
    // this project.
    #[structopt(help = "Name(s) of the project(s) to query")]
    proj_names: Vec<String>,
}

impl Command for ShowVersionCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        sess.populated_graph()?;

        // Get the list of projects that we're interested in.
        //
        // TODO: better validation and more flexible querying; if no names are
        // provided, default to staging any changed projects.
        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let idents = sess.graph().query_ids(q)?;

        if idents.len() != 1 {
            println!("error: must specify exactly one project to show");
            return Ok(1);
        }

        let proj = sess.graph().lookup(idents[0]);
        println!("{}", proj.version);
        Ok(0)
    }
}

// stage

#[derive(Debug, PartialEq, StructOpt)]
struct StageCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force staging even in unexpected conditions"
    )]
    force: bool,

    #[structopt(help = "Name(s) of the project(s) to stage for release")]
    proj_names: Vec<String>,
}

impl Command for StageCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        sess.populated_graph()?;

        if let Err(e) = sess.repo.check_dirty() {
            if self.force {
                warn!("staging despite dirty repo due to --force");
            } else {
                return Err(e).context("refusing to stage (override with `--force`)");
            }
        }

        match sess.execution_environment() {
            app::ExecutionEnvironment::NotCi => {}

            _ => {
                warn!("`cranko stage` seems to be running in a CI environment; this is not recommended");
            }
        }

        // Get the list of projects that we're interested in.
        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let empty_query = q.is_empty();
        let idents = sess
            .graph()
            .query_or_all(q)
            .context("could not select projects for staging")?;

        if idents.len() == 0 {
            info!("no projects selected");
            return Ok(0);
        }

        // Pull up the relevant repository history for all of those projects.
        let history = {
            let graph = sess.graph();
            let mut matchers = Vec::new();

            for projid in &idents {
                let proj = graph.lookup(*projid);
                matchers.push(&proj.repo_paths);
            }

            sess.repo.analyze_history_to_release(&matchers[..])?
        };

        // Update the changelogs
        let mut n_staged = 0;
        let rel_info = sess.repo.get_latest_release_info()?;

        for i in 0..idents.len() {
            let proj = sess.graph().lookup(idents[i]);
            let changes = &history[i][..];

            if changes.len() == 0 {
                if !empty_query {
                    warn!("no changes detected for project {}", proj.user_facing_name);
                }
            } else {
                println!(
                    "{}: {} relevant commits",
                    proj.user_facing_name,
                    changes.len()
                );
                proj.changelog
                    .draft_release_update(proj, &sess, changes, rel_info.commit)?;
                n_staged += 1;
            }
        }

        if empty_query && n_staged != 1 {
            info!("{} of {} projects staged", n_staged, idents.len());
        } else if n_staged != idents.len() {
            info!("{} of {} selected projects staged", n_staged, idents.len());
        }

        Ok(0)
    }
}

// status

#[derive(Debug, PartialEq, StructOpt)]
struct StatusCommand {
    #[structopt(help = "Name(s) of the project(s) to query (default: all)")]
    proj_names: Vec<String>,
}

impl Command for StatusCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        sess.populated_graph()?;

        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let idents = sess
            .graph()
            .query_or_all(q)
            .context("cannot get requested statuses")?;

        let rci = sess.repo.get_latest_release_info()?;
        let oids = sess.analyze_history_to_release()?;

        for ident in idents {
            let n = oids[ident].len();
            let proj = sess.graph().lookup(ident);

            if let Some(latest) = rci.lookup_project(proj) {
                println!(
                    "{}: {} relevant commit(s) since {}",
                    proj.user_facing_name, n, latest.version
                );
            } else {
                println!(
                    "{}: {} relevant commit(s) since start of history (no releases on record)",
                    proj.user_facing_name, n
                );
            }
        }

        Ok(0)
    }
}

/// Run an external command by executing a subprocess.
fn do_external(all_args: Vec<String>) -> Result<i32> {
    let (cmd, args) = all_args.split_first().unwrap();

    let command_exe = format!("cranko-{}{}", cmd, env::consts::EXE_SUFFIX);
    let path = search_directories()
        .iter()
        .map(|dir| dir.join(&command_exe))
        .find(|file| is_executable(file));

    let command = path.ok_or_else(|| errors::CliError::NoSuchSubcommand(cmd.to_owned()))?;
    exec_or_spawn(std::process::Command::new(command).args(args))
}

#[cfg(unix)]
/// On Unix, exec() to replace ourselves with the child process. This function
/// *should* never return.
fn exec_or_spawn(cmd: &mut std::process::Command) -> Result<i32> {
    use std::os::unix::process::CommandExt;

    // exec() only returns an io::Error directly, since on success it never
    // returns; the following tomfoolery transforms it into our Result
    // machinery as desired.
    Ok(Err(cmd.exec())?)
}

#[cfg(not(unix))]
/// On other platforms, just run the process and wait for it.
fn exec_or_spawn(cmd: &mut std::process::Command) -> Result<i32> {
    // code() can only return None on Unix when the subprocess was killed by a
    // signal. This function only runs if we're not on Unix, so we'll always
    // get Some.
    Ok(cmd.status()?.code().unwrap())
}

// Lots of copy/paste from cargo:

fn list_commands() -> BTreeSet<String> {
    let prefix = "cranko-";
    let suffix = env::consts::EXE_SUFFIX;
    let mut commands = BTreeSet::new();

    for dir in search_directories() {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            _ => continue,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let filename = match path.file_name().and_then(|s| s.to_str()) {
                Some(filename) => filename,
                _ => continue,
            };
            if !filename.starts_with(prefix) || !filename.ends_with(suffix) {
                continue;
            }
            if is_executable(entry.path()) {
                let end = filename.len() - suffix.len();
                commands.insert(filename[prefix.len()..end].to_string());
            }
        }
    }

    commands.insert("confirm".to_owned());
    commands.insert("github".to_owned());
    commands.insert("help".to_owned());
    commands.insert("list-commands".to_owned());
    commands.insert("release-workflow".to_owned());
    commands.insert("show".to_owned());
    commands.insert("stage".to_owned());
    commands.insert("status".to_owned());

    commands
}

#[cfg(unix)]
fn is_executable<P: AsRef<Path>>(path: P) -> bool {
    use std::os::unix::prelude::*;
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable<P: AsRef<Path>>(path: P) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn search_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(val) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&val));
    }
    dirs
}
