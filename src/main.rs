// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! The main cranko command-line interface.
//!
//! This just provides swiss-army-knife access to commands installed by other
//! Cranko modules. Not 100% sure this is the way to got but we'll see.
//!
//! Heavily modeled on Cargo's implementation of the same sort of functionality.

use anyhow::Result;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};
use structopt::StructOpt;

mod app;
mod changelog;
mod errors;
mod graph;
mod loaders;
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
    #[structopt(name = "apply")]
    /// Create a new commit applying version numbers all projects
    Apply(ApplyCommand),

    #[structopt(name = "confirm")]
    /// Commit staged release requests to the `rc` branch
    Confirm(ConfirmCommand),

    #[structopt(name = "help")]
    /// Prints this message or the help of the given subcommand
    Help(HelpCommand),

    #[structopt(name = "list-commands")]
    /// List available subcommands
    ListCommands(ListCommandsCommand),

    #[structopt(name = "show")]
    /// Print out various useful pieces of information.
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
            Commands::Apply(o) => o.execute(),
            Commands::Confirm(o) => o.execute(),
            Commands::Help(o) => o.execute(),
            Commands::ListCommands(o) => o.execute(),
            Commands::Show(o) => o.execute(),
            Commands::Stage(o) => o.execute(),
            Commands::Status(o) => o.execute(),
            Commands::External(args) => do_external(args),
        }
    }
}

fn main() -> Result<()> {
    let opts = CrankoOptions::from_args();
    let exitcode = opts.command.execute()?;
    std::process::exit(exitcode);
}

// apply

#[derive(Debug, PartialEq, StructOpt)]
struct ApplyCommand {}

impl Command for ApplyCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        let info = ci_info::get();
        let mut rci = None;

        if info.ci {
            if let Some(branch_name) = info.branch_name {
                if branch_name == sess.repo.upstream_rc_name() {
                    println!("computing new versions based on `rc` commit request data");
                    rci = Some(sess.repo.parse_rc_info_from_head()?);
                }
            }
        }

        let rci = rci.unwrap_or_else(|| {
            println!("computing new verions assuming development mode");
            sess.default_dev_rc_info()
        });

        sess.apply_versions(&rci)?;
        let mut changes = sess.rewrite()?;
        sess.apply_changelogs(&rci, &mut changes)?;
        sess.make_release_commit(&changes)?;
        Ok(0)
    }
}

// confirm

#[derive(Debug, PartialEq, StructOpt)]
struct ConfirmCommand {}

impl Command for ConfirmCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        sess.populated_graph()?;

        let mut changes = repository::ChangeList::default();
        let mut rcinfo = Vec::new();

        for proj in sess.graph().toposort()? {
            if let Some(info) = sess.repo.scan_rc_info(proj, &mut changes)? {
                rcinfo.push(info);
            }
        }

        if rcinfo.len() < 1 {
            println!("no releases seem to have been staged; use \"cranko stage\"?");
            return Ok(0);
        }

        sess.make_rc_commit(rcinfo, &changes)?;
        println!("staged rc commit to \"rc\" branch");

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
    #[structopt(help = "Name(s) of the project(s) to stage for release")]
    proj_names: Vec<String>,
}

impl Command for StageCommand {
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
        for i in 0..idents.len() {
            let proj = sess.graph().lookup(idents[i]);
            let changes = &history[i][..];
            proj.changelog.draft_release_update(proj, &sess, changes)?;
        }

        Ok(0)
    }
}

// status

#[derive(Debug, PartialEq, StructOpt)]
struct StatusCommand {}

impl Command for StatusCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        sess.populated_graph()?;
        let oids = sess.analyze_history_to_release()?;
        let graph = sess.graph();

        for proj in graph.toposort()? {
            println!("{}: {}", proj.user_facing_name, proj.version);
            println!(
                "  number of relevant commits since release: {}",
                oids[proj.ident()].len()
            );
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

    commands.insert("apply".to_owned());
    commands.insert("confirm".to_owned());
    commands.insert("help".to_owned());
    commands.insert("list-commands".to_owned());
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
