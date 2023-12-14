// Copyright 2020-2022 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! The main cranko command-line interface.
//!
//! This just provides swiss-army-knife access to commands installed by other
//! Cranko modules. Not 100% sure this is the way to got but we'll see.
//!
//! Heavily modeled on Cargo's implementation of the same sort of functionality.

use anyhow::{anyhow, bail, Context};
use log::{info, warn};
use std::{
    collections::BTreeSet,
    env as stdenv,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process,
};
use structopt::StructOpt;

mod app;
mod bootstrap;
mod cargo;
mod changelog;
mod config;
mod csproj;
mod env;
mod errors;
mod github;
mod gitutil;
mod graph;
mod logger;
mod npm;
mod project;
mod pypa;
mod repository;
mod rewriters;
mod version;
mod zenodo;

use errors::Result;

#[derive(Debug, PartialEq, StructOpt)]
#[structopt(about = "automate versioning and releasing")]
struct CrankoOptions {
    #[structopt(subcommand)]
    command: Commands,
}

trait Command {
    fn execute(self) -> Result<i32>;
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, PartialEq, StructOpt)]
enum Commands {
    #[structopt(name = "bootstrap")]
    /// Bootstrap Cranko in a preexisting repository
    Bootstrap(bootstrap::BootstrapCommand),

    #[structopt(name = "cargo")]
    /// Commands specific to the Rust/Cargo packaging system.
    Cargo(cargo::CargoCommand),

    #[structopt(name = "ci-util")]
    /// Utilities useful in CI environments
    CiUtil(CiUtilCommand),

    #[structopt(name = "confirm")]
    /// Commit staged release requests to the `rc` branch
    Confirm(ConfirmCommand),

    #[structopt(name = "diff")]
    /// Show "diff" output since the latest release
    Diff(DiffCommand),

    #[structopt(name = "github")]
    /// GitHub release utilities
    Github(github::GithubCommand),

    #[structopt(name = "git-util")]
    /// Specialized Git utilities
    GitUtil(gitutil::GitUtilCommand),

    #[structopt(name = "help")]
    /// Prints this message or the help of the given subcommand
    Help(HelpCommand),

    #[structopt(name = "list-commands")]
    /// List available subcommands
    ListCommands(ListCommandsCommand),

    #[structopt(name = "log")]
    /// Show the version control log for a specific project
    Log(LogCommand),

    #[structopt(name = "npm")]
    /// Commands specific to the NPM packaging system.
    Npm(npm::NpmCommand),

    #[structopt(name = "python")]
    /// Commands related to the Python programming language.
    Python(pypa::PythonCommand),

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

    #[structopt(name = "zenodo")]
    /// Zenodo deposition utilities
    Zenodo(zenodo::ZenodoCommand),

    #[structopt(external_subcommand)]
    External(Vec<String>),
}

impl Command for Commands {
    fn execute(self) -> Result<i32> {
        match self {
            Commands::Bootstrap(o) => o.execute(),
            Commands::Cargo(o) => o.execute(),
            Commands::CiUtil(o) => o.execute(),
            Commands::Confirm(o) => o.execute(),
            Commands::Diff(o) => o.execute(),
            Commands::Github(o) => o.execute(),
            Commands::GitUtil(o) => o.execute(),
            Commands::Help(o) => o.execute(),
            Commands::ListCommands(o) => o.execute(),
            Commands::Log(o) => o.execute(),
            Commands::Npm(o) => o.execute(),
            Commands::Python(o) => o.execute(),
            Commands::ReleaseWorkflow(o) => o.execute(),
            Commands::Show(o) => o.execute(),
            Commands::Stage(o) => o.execute(),
            Commands::Status(o) => o.execute(),
            Commands::Zenodo(o) => o.execute(),
            Commands::External(args) => do_external(args),
        }
    }
}

fn main() {
    let opts = CrankoOptions::from_args();

    if let Err(e) = logger::Logger::init() {
        eprintln!("error: cannot initialize logging backend: {}", e);
        process::exit(1);
    }
    log::set_max_level(log::LevelFilter::Info);

    process::exit(errors::report(opts.command.execute()));
}

// ci-util

#[derive(Debug, PartialEq, StructOpt)]
struct CiUtilCommand {
    #[structopt(subcommand)]
    command: CiUtilCommands,
}

#[derive(Debug, PartialEq, StructOpt)]
enum CiUtilCommands {
    #[structopt(name = "env-to-file")]
    /// Save an environment variable to a file
    EnvToFile(CiUtilEnvToFileCommand),
}

impl Command for CiUtilCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            CiUtilCommands::EnvToFile(o) => o.execute(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum EnvDecodingMode {
    /// The value is interpreted as text and written out as UTF8.
    Text,

    /// The value is encoded in the variable in base64 format.
    Base64,
}

impl std::str::FromStr for EnvDecodingMode {
    type Err = errors::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "text" => Ok(EnvDecodingMode::Text),
            "base64" => Ok(EnvDecodingMode::Base64),
            _ => Err(anyhow!("unrecognized encoding mode `{}`", s)),
        }
    }
}

#[derive(Debug, PartialEq, StructOpt)]
struct CiUtilEnvToFileCommand {
    #[structopt(
        long = "decode",
        default_value = "text",
        help = "How to decode the variable value into bytes"
    )]
    decode_mode: EnvDecodingMode,

    #[structopt(help = "Name of the environment variable")]
    var_name: OsString,

    #[structopt(help = "The destination file name")]
    file_name: PathBuf,
}

impl Command for CiUtilEnvToFileCommand {
    fn execute(self) -> Result<i32> {
        use std::fs::OpenOptions;

        // Get the variable value.
        let value = stdenv::var_os(&self.var_name).ok_or_else(|| {
            anyhow!(
                "environment variable `{}` not available",
                &self.var_name.to_string_lossy()
            )
        })?;

        // Set up to create the file, as securely as we can manage. AFAICT,
        // there aren't any Windows options that help here?
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);

        #[cfg(unix)]
        fn platform_options(o: &mut OpenOptions) {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }

        #[cfg(not(unix))]
        fn platform_options(_o: &mut OpenOptions) {}

        platform_options(&mut options);

        let mut file = options.open(&self.file_name).with_context(|| {
            format!(
                "cannot securely open `{}` for writing",
                self.file_name.display()
            )
        })?;

        // Write the data. Eventually we might have more options, but for now we
        // always interpret the OsString into text, then convert it into a
        // Vec<[u8]> for writing.

        let value = value.into_string().map_err(|_| {
            anyhow!(
                "cannot interpret value of environment variable `{}` as Unicode text",
                self.var_name.to_string_lossy()
            )
        })?;

        let b = match self.decode_mode {
            EnvDecodingMode::Text => value.into_bytes(),

            EnvDecodingMode::Base64 => base64::decode(&value).with_context(|| {
                format!(
                    "failed to decode value of environment variable `{}` as BASE64",
                    self.var_name.to_string_lossy()
                )
            })?,
        };

        file.write_all(&b[..]).with_context(|| {
            format!(
                "failed trying to write data to file `{}`",
                self.file_name.display()
            )
        })?;

        Ok(0)
    }
}

// confirm

#[derive(Debug, PartialEq, StructOpt)]
struct ConfirmCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force operation even in unexpected conditions"
    )]
    force: bool,
}

impl Command for ConfirmCommand {
    fn execute(self) -> Result<i32> {
        use project::DepRequirement;

        let mut sess = app::AppSession::initialize_default()?;
        sess.ensure_not_ci(self.force)?;

        if let Err(e) = sess.ensure_changelog_clean() {
            warn!(
                "not recommended to confirm with a modified working tree ({})",
                e
            );
            if !self.force {
                bail!("refusing to proceed (use `--force` to override)");
            }
        }

        // Scan the repository histories for everybody -- we'll use these to
        // report whether there are projects that ought to be released but
        // aren't.
        let histories = sess.analyze_histories()?;
        let mut changes = repository::ChangeList::default();
        let mut rc_info = Vec::new();

        sess.solve_internal_deps(|repo, graph, ident| {
            let history = histories.lookup(ident);
            let dirty_allowed = self.force;
            let mut updated_version = false;

            if let Some(info) =
                repo.scan_rc_info(graph.lookup(ident), &mut changes, dirty_allowed)?
            {
                // Analyze the version bump and apply it (in-memory only).

                let (old_version_text, new_version) = {
                    let proj = graph.lookup_mut(ident);
                    let last_rel_info = history.release_info(repo)?;
                    let scheme = proj.version.parse_bump_scheme(&info.bump_spec)?;

                    if let Some(last_release) = last_rel_info.lookup_project(proj) {
                        proj.version = proj.version.parse_like(&last_release.version)?;
                        scheme.apply(&mut proj.version)?;
                        (last_release.version.clone(), proj.version.clone())
                    } else {
                        scheme.apply(&mut proj.version)?;
                        ("[no previous releases]".to_owned(), proj.version.clone())
                    }
                };

                let proj = graph.lookup(ident);

                if history.n_commits() == 0 {
                    warn!(
                        "project `{}` is being staged for release, but does not \
                        seem to have been modified since its last release",
                        proj.user_facing_name
                    );
                }

                info!(
                    "{}: {} (expected: {} => {})",
                    proj.user_facing_name, info.bump_spec, old_version_text, new_version
                );
                rc_info.push(info);
                updated_version = true;

                for dep in &proj.internal_deps[..] {
                    let dproj = graph.lookup(dep.ident);
                    let req_text = match &dep.cranko_requirement {
                        DepRequirement::Commit(_) => {
                            format!(">= {}", dep.resolved_version.as_ref().unwrap())
                        }
                        DepRequirement::Manual(t) => format!("{} (manual)", t),
                        DepRequirement::Unavailable => {
                            "** version requirement unavailable **".to_owned()
                        }
                    };

                    info!("    internal dep {}: {}", dproj.user_facing_name, req_text);
                }
            } else if history.n_commits() > 0 {
                warn!(
                    "project `{}` has been changed since its last release, \
                    but is not part of the rc submission",
                    graph.lookup(ident).user_facing_name
                );
            }

            Ok(updated_version)
        })?;

        if rc_info.is_empty() {
            warn!("no releases seem to have been staged; use \"cranko stage\"?");
            return Ok(0);
        }

        sess.make_rc_commit(rc_info, &changes)?;
        info!(
            "staged rc commit to `{}` branch",
            sess.repo.upstream_rc_name()
        );

        sess.repo.hard_reset_changes(&changes)?;
        Ok(0)
    }
}

// diff

#[derive(Debug, PartialEq, StructOpt)]
struct DiffCommand {
    #[structopt(help = "Name of the project to query")]
    proj_names: Vec<String>,
}

impl Command for DiffCommand {
    fn execute(self) -> Result<i32> {
        // See also "log" -- these follow similar patterns
        let sess = app::AppSession::initialize_default()?;

        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let idents = sess.graph().query(q)?;
        if idents.len() != 1 {
            bail!("must specify exactly one project to diff");
        }
        let ident = idents[0];

        let dir = sess
            .repo
            .resolve_workdir(sess.graph().lookup(ident).prefix());

        let histories = atry!(
            sess.analyze_histories();
            ["failed to analyze the repository history"]
        );

        let history = histories.lookup(ident);

        let commit = match history.main_branch_commit(&sess.repo)? {
            Some(c) => c,
            None => {
                println!(
                    "no known last release commit to diff against for `{}`",
                    sess.graph().lookup(ident).user_facing_name
                );
                return Ok(0);
            }
        };

        // For now, just launch "git" as a command.

        let mut cmd = process::Command::new("git");
        cmd.arg("diff");
        cmd.arg(&commit.to_string()[..8]);
        cmd.arg("--");
        cmd.arg(dir);
        exec_or_spawn(&mut cmd)
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
                CrankoOptions::from_iter(&[&stdenv::args().next().unwrap(), cmd, "--help"])
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

// log

#[derive(Debug, PartialEq, StructOpt)]
struct LogCommand {
    #[structopt(long = "stat", help = "Show a diffstat with each commit")]
    stat: bool,

    #[structopt(help = "Name of the project to query")]
    proj_names: Vec<String>,
}

impl Command for LogCommand {
    fn execute(self) -> Result<i32> {
        // See also "diff" -- these follow similar patterns
        let sess = app::AppSession::initialize_default()?;

        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let idents = sess.graph().query(q)?;
        if idents.len() != 1 {
            bail!("must specify exactly one project to log");
        }
        let ident = idents[0];

        let histories = atry!(
            sess.analyze_histories();
            ["failed to analyze the repository history"]
        );

        let history = histories.lookup(ident);

        if history.n_commits() == 0 {
            println!(
                "no relevant commits to show for `{}`",
                sess.graph().lookup(ident).user_facing_name
            );
            return Ok(0);
        }

        // I think the most sensible thing to do here is just launch `git` as a
        // command. Note, however, that we might in principle be installed
        // somewhere where the Git CLI isn't actually available.

        let mut cmd = process::Command::new("git");
        cmd.arg("show");

        if self.stat {
            cmd.arg("--stat");
        } else {
            cmd.arg("--no-patch");
        }

        for cid in history.commits() {
            cmd.arg(&cid.to_string()[..8]);
        }

        exec_or_spawn(&mut cmd)
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
        let mut sess = app::AppSession::initialize_default()?;
        sess.ensure_fully_clean()?;

        let (dev_mode, rci) = sess.ensure_ci_rc_mode(self.force)?;
        if dev_mode {
            info!("computing new versions for \"development\" mode");
        } else {
            info!("computing new versions based on `rc` commit request data");
        }

        let rel_info = sess.repo.get_latest_release_info()?;

        sess.apply_versions(&rci)?;
        let mut changes = sess.rewrite()?;

        if !dev_mode {
            sess.apply_changelogs(rel_info.commit, &rci, &mut changes)?;
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
        let mut sess = app::AppSession::initialize_default()?;

        // We won't complain if people want to make a release commit on updates
        // to `master` or whatever: they might want to monitor that that part of
        // the workflow seems to be in good working order. Just so long as they
        // don't *push* that commit at the wrong time, it's OK.
        let (_dev, rci) = sess.ensure_ci_rc_mode(self.force)?;
        sess.make_release_commit(&rci)?;
        Ok(0)
    }
}

// release-workflow tag

#[derive(Debug, PartialEq, StructOpt)]
struct ReleaseWorkflowTagCommand {}

impl Command for ReleaseWorkflowTagCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = app::AppSession::initialize_default()?;
        let (dev_mode, rel_info) = sess.ensure_ci_release_mode()?;

        if dev_mode {
            bail!("refusing to create tags in dev mode");
        }

        sess.create_tags(&rel_info)?;
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
    #[structopt(name = "cranko-version-doi")]
    /// Print the DOI associated with this specific version of Cranko.
    CrankoVersionDoi(ShowCrankoVersionDoiCommand),

    #[structopt(name = "cranko-concept-doi")]
    /// Print the DOI uniting all versions of the Cranko software package.
    CrankoConceptDoi(ShowCrankoConceptDoiCommand),

    #[structopt(name = "if-released")]
    /// Report if a project was just released
    IfReleased(ShowIfReleasedCommand),

    #[structopt(name = "tctag")]
    /// Print a "thiscommit:" tag for copy/pasting
    TcTag(ShowTcTagCommand),

    #[structopt(name = "toposort")]
    /// Print the projects in topologically-sorted order
    Toposort(ShowToposortCommand),

    #[structopt(name = "version")]
    /// Print the current version number of a project
    Version(ShowVersionCommand),
}

impl Command for ShowCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            ShowCommands::CrankoVersionDoi(o) => o.execute(),
            ShowCommands::CrankoConceptDoi(o) => o.execute(),
            ShowCommands::IfReleased(o) => o.execute(),
            ShowCommands::TcTag(o) => o.execute(),
            ShowCommands::Toposort(o) => o.execute(),
            ShowCommands::Version(o) => o.execute(),
        }
    }
}

#[derive(Debug, PartialEq, StructOpt)]
struct ShowCrankoVersionDoiCommand {}

impl Command for ShowCrankoVersionDoiCommand {
    fn execute(self) -> Result<i32> {
        // For releases, this will be rewritten to the real DOI:
        let doi = "10.5281/zenodo.10382647";

        if doi.starts_with("xx.") {
            warn!("you are running a development build; the printed value is not a real DOI");
        }

        println!("{}", doi);
        Ok(0)
    }
}

#[derive(Debug, PartialEq, StructOpt)]
struct ShowCrankoConceptDoiCommand {}

impl Command for ShowCrankoConceptDoiCommand {
    fn execute(self) -> Result<i32> {
        // For releases, this will be rewritten to the real DOI:
        let doi = "10.5281/zenodo.6981679";

        if doi.starts_with("xx.") {
            warn!("you are running a development build; the printed value is not a real DOI");
        }

        println!("{}", doi);
        Ok(0)
    }
}

#[derive(Debug, PartialEq, StructOpt)]
struct ShowIfReleasedCommand {
    #[structopt(
        long = "exit-code",
        help = "Exit the program with success if released, failure if not"
    )]
    exit_code: bool,

    #[structopt(long = "tf", help = "Print \"true\" if released, \"false\" if not")]
    true_false: bool,

    #[structopt(help = "Name of the project to query")]
    proj_names: Vec<String>,
}

impl Command for ShowIfReleasedCommand {
    fn execute(self) -> Result<i32> {
        let sess = app::AppSession::initialize_default()?;

        if !(self.exit_code || self.true_false) {
            bail!("must specify at least one output mechanism");
        }

        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let idents = sess.graph().query(q)?;

        if idents.len() != 1 {
            bail!("must specify exactly one project to show");
        }

        let (_dev_mode, rel_info) = sess.ensure_ci_release_mode()?;

        let proj = sess.graph().lookup(idents[0]);
        let was_released = rel_info.lookup_if_released(proj).is_some();

        if self.true_false {
            println!("{}", if was_released { "true" } else { "false" });
        }

        Ok(if self.exit_code {
            if was_released {
                0
            } else {
                1
            }
        } else {
            0
        })
    }
}

#[derive(Debug, PartialEq, StructOpt)]
struct ShowTcTagCommand {}

impl Command for ShowTcTagCommand {
    fn execute(self) -> Result<i32> {
        use chrono::prelude::*;
        use rand::{distributions::Alphanumeric, Rng};

        let utc: DateTime<Utc> = Utc::now();

        let mut rng = rand::thread_rng();
        let chars: String = std::iter::repeat(())
            .map(|()| rng.sample(Alphanumeric))
            .map(char::from)
            .take(7)
            .collect();

        println!(
            "thiscommit:{:>04}-{:>02}-{:>02}:{}",
            utc.year(),
            utc.month(),
            utc.day(),
            chars
        );
        Ok(0)
    }
}

#[derive(Debug, PartialEq, StructOpt)]
struct ShowToposortCommand {}

impl Command for ShowToposortCommand {
    fn execute(self) -> Result<i32> {
        let sess = app::AppSession::initialize_default()?;
        let graph = sess.graph();

        for ident in graph.toposorted() {
            let proj = graph.lookup(ident);
            println!("{}", proj.user_facing_name);
        }

        Ok(0)
    }
}

// TODO: add something like `--ifdev=latest` to print "latest"
// instead of 0.0.0-dev.0 if we're not on a release commit for
// this project.
#[derive(Debug, PartialEq, StructOpt)]
struct ShowVersionCommand {
    #[structopt(help = "Name of the project to query")]
    proj_names: Vec<String>,
}

impl Command for ShowVersionCommand {
    fn execute(self) -> Result<i32> {
        let sess = app::AppSession::initialize_default()?;

        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let idents = sess.graph().query(q)?;

        if idents.len() != 1 {
            bail!("must specify exactly one project to show");
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
        let sess = app::AppSession::initialize_default()?;

        if let Err(e) = sess.ensure_changelog_clean() {
            warn!(
                "not recommended to stage with a modified working tree ({})",
                e
            );
            if !self.force {
                bail!("refusing to proceed (use `--force` to override)");
            }
        }

        sess.ensure_not_ci(self.force)?;

        // Get the list of projects that we're interested in.
        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let no_names = q.no_names();
        let idents = sess
            .graph()
            .query(q)
            .context("could not select projects for staging")?;

        if idents.is_empty() {
            info!("no projects selected");
            return Ok(0);
        }

        // Scan the repository histories for everybody.
        let histories = sess.analyze_histories()?;

        // Update the changelogs
        let mut n_staged = 0;
        let rel_info = sess.repo.get_latest_release_info()?;
        let mut changes = repository::ChangeList::default();

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);
            let history = histories.lookup(*ident);
            let dirty_allowed = self.force;

            if sess
                .repo
                .scan_rc_info(proj, &mut changes, dirty_allowed)?
                .is_some()
            {
                if !no_names {
                    warn!(
                        "skipping {}: it appears to have already been staged",
                        proj.user_facing_name
                    );
                }
                continue;
            }

            // We selected this project but don't stage it if:
            // - there are no new commits AND EITHER
            //   - we're not in force-mode OR
            //   - we only selected it because we're in "no-specific-names" mode
            if (no_names || !self.force) && history.n_commits() == 0 {
                if !no_names {
                    warn!("no changes detected for project {}", proj.user_facing_name);
                }
            } else {
                println!(
                    "{}: {} relevant commits",
                    proj.user_facing_name,
                    history.n_commits()
                );

                // Because Changelog is a boxed trait object, it can't accept
                // generic types :-(
                let commits: Vec<repository::CommitId> =
                    history.commits().into_iter().copied().collect();
                proj.changelog
                    .draft_release_update(proj, &sess, &commits[..], rel_info.commit)?;
                n_staged += 1;
            }
        }

        if no_names && n_staged == 0 {
            info!("nothing further to stage at this time");
        } else if no_names && n_staged != 1 {
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
        let sess = app::AppSession::initialize_default()?;

        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let idents = sess
            .graph()
            .query(q)
            .context("cannot get requested statuses")?;

        let histories = sess.analyze_histories()?;

        for ident in idents {
            let proj = sess.graph().lookup(ident);
            let history = histories.lookup(ident);
            let n = history.n_commits();
            let rel_info = history.release_info(&sess.repo)?;

            if let Some(this_info) = rel_info.lookup_project(proj) {
                if this_info.age == 0 {
                    if n == 0 {
                        println!(
                            "{}: no relevant commits since {}",
                            proj.user_facing_name, this_info.version
                        );
                    } else {
                        logger::Logger::println_highlighted(
                            format!("{}: ", proj.user_facing_name),
                            n,
                            format!(" relevant commit(s) since {}", this_info.version),
                        );
                    }
                } else {
                    logger::Logger::println_highlighted(
                        format!("{}: no more than ", proj.user_facing_name),
                        n,
                        format!(
                            " relevant commit(s) since {} (unable to track in detail)",
                            this_info.version
                        ),
                    );
                }
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

#[allow(clippy::redundant_closure)]
/// Run an external command by executing a subprocess.
fn do_external(all_args: Vec<String>) -> Result<i32> {
    let (cmd, args) = all_args.split_first().unwrap();

    let command_exe = format!("cranko-{}{}", cmd, stdenv::consts::EXE_SUFFIX);
    let path = search_directories()
        .iter()
        .map(|dir| dir.join(&command_exe))
        .find(|file| is_executable(file));

    let command = path.ok_or_else(|| {
        anyhow!(
            "no internal or external subcommand `{0}` is available (install `cranko-{0}`?)",
            cmd
        )
    })?;
    exec_or_spawn(process::Command::new(command).args(args))
}

#[cfg(unix)]
/// On Unix, exec() to replace ourselves with the child process. This function
/// *should* never return.
fn exec_or_spawn(cmd: &mut process::Command) -> Result<i32> {
    use std::os::unix::process::CommandExt;

    // exec() only returns an io::Error directly, since on success it never
    // returns; the following tomfoolery transforms it into our Result
    // machinery as desired.
    Err(cmd.exec().into())
}

#[cfg(not(unix))]
/// On other platforms, just run the process and wait for it.
fn exec_or_spawn(cmd: &mut process::Command) -> Result<i32> {
    // code() can only return None on Unix when the subprocess was killed by a
    // signal. This function only runs if we're not on Unix, so we'll always
    // get Some.
    Ok(cmd.status()?.code().unwrap())
}

// Lots of copy/paste from cargo:

fn list_commands() -> BTreeSet<String> {
    let prefix = "cranko-";
    let suffix = stdenv::consts::EXE_SUFFIX;
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

    commands.insert("bootstrap".to_owned());
    commands.insert("cargo".to_owned());
    commands.insert("ci-util".to_owned());
    commands.insert("confirm".to_owned());
    commands.insert("git-util".to_owned());
    commands.insert("github".to_owned());
    commands.insert("help".to_owned());
    commands.insert("list-commands".to_owned());
    commands.insert("log".to_owned());
    commands.insert("npm".to_owned());
    commands.insert("python".to_owned());
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

    if let Some(val) = stdenv::var_os("PATH") {
        dirs.extend(stdenv::split_paths(&val));
    }
    dirs
}

// I tried to set up the line-ending character as a macro that evaluated to a string
// literal, but couldn't get the macro imports to work, for some reason along the
// lines of https://github.com/rust-lang/rust/issues/57966.

#[cfg(not(windows))]
#[macro_export]
macro_rules! write_crlf {
    ($stream:expr, $format:literal $($rest:tt)*) => {{
        use std::io::Write;
        write!($stream, $format $($rest)*).and_then(|_x| write!($stream, "\n"))
    }}
}

#[cfg(windows)]
#[macro_export]
macro_rules! write_crlf {
    ($stream:expr, $format:literal $($rest:tt)*) => {{
        use std::io::Write;
        write!($stream, $format $($rest)*).and_then(|_x| write!($stream, "\r\n"))
    }}
}
