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

    #[structopt(name = "help")]
    /// Prints this message or the help of the given subcommand
    Help(HelpCommand),

    #[structopt(name = "list-commands")]
    /// List available subcommands
    ListCommands(ListCommandsCommand),

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
            Commands::Help(o) => o.execute(),
            Commands::ListCommands(o) => o.execute(),
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
        sess.apply_versions(version::ReleaseMode::Development)?;
        let changes = sess.rewrite()?;
        sess.repo.make_release_commit(&changes)?;
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

// status

#[derive(Debug, PartialEq, StructOpt)]
struct StatusCommand {}

impl Command for StatusCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize()?;
        let graph = sess.populated_graph()?;

        for proj in graph.toposort()? {
            println!("{}: {}", proj.user_facing_name(), proj.version);
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
    commands.insert("help".to_owned());
    commands.insert("list-commands".to_owned());
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
