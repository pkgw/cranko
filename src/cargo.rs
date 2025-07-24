// Copyright 2020-2021 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Cargo (Rust) projects.
//!
//! If we detect a Cargo.toml in the repo root, we use `cargo metadata` to slurp
//! information about all of the crates and their interdependencies.

use anyhow::{anyhow, Context};
use cargo_metadata::MetadataCommand;
use log::{info, warn};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs::File,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    process, thread, time,
};
use structopt::StructOpt;
use toml_edit::{Document, Item, Table};

use super::Command;

use crate::{
    app::{AppBuilder, AppSession},
    atry,
    config::ProjectConfiguration,
    errors::Result,
    graph::GraphQueryBuilder,
    project::{DepRequirement, DependencyTarget, Project, ProjectId},
    repository::{ChangeList, RepoPath, RepoPathBuf},
    rewriters::Rewriter,
    version::Version,
};

/// Framework for auto-loading Cargo projects from the repository contents.
#[derive(Debug, Default)]
pub struct CargoLoader {
    shortest_toml_dirname: Option<RepoPathBuf>,
}

impl CargoLoader {
    /// Process items in the Git index while auto-loading projects. Since we use
    /// `cargo metadata` to get project information, all we do here is find the
    /// toplevel `Cargo.toml` file and assume that it represents a single
    /// project root, as far as Cargo is concerned. If you have some weird repo
    /// structure that doesn't have a single toplevel Cargo.toml (either a
    /// workspace, or a single project), we'll have trouble with that.
    pub fn process_index_item(&mut self, dirname: &RepoPath, basename: &RepoPath) {
        if basename.as_ref() != b"Cargo.toml" {
            return;
        }

        if let Some(ref mut prev) = self.shortest_toml_dirname {
            // Find the longest common prefix of the two dirnames.
            let bytes0: &[u8] = prev.as_ref();
            let bytes1: &[u8] = dirname.as_ref();
            let len = bytes0
                .iter()
                .zip(bytes1)
                .take_while(|&(a, b)| a == b)
                .count();
            prev.truncate(len);
        } else {
            self.shortest_toml_dirname = Some(dirname.to_owned());
        }
    }

    /// Finalize autoloading any Cargo projects. Consumes this object.
    ///
    /// If this repository contains one or more `Cargo.toml` files, the
    /// `cargo_metadata` crate will be used to load project information.
    pub fn finalize(
        self,
        app: &mut AppBuilder,
        pconfig: &HashMap<String, ProjectConfiguration>,
    ) -> Result<()> {
        let shortest_toml_dirname = match self.shortest_toml_dirname {
            Some(d) => d,
            None => return Ok(()),
        };

        let mut toml_path = app.repo.resolve_workdir(&shortest_toml_dirname);
        toml_path.push("Cargo.toml");
        let mut cmd = MetadataCommand::new();
        cmd.manifest_path(&toml_path);
        cmd.features(cargo_metadata::CargoOpt::AllFeatures);
        let cargo_meta = atry!(
            cmd.exec();
            ["failed to fetch Cargo metadata using the `cargo metadata` command"]
        );

        // Fill in the packages

        let mut cargo_to_graph = HashMap::new();

        for pkg in &cargo_meta.packages {
            if pkg.source.is_some() {
                continue; // This is an external package; not to be tracked.
            }

            // Plan to auto-register a rewriter to update this package's Cargo.toml.
            let manifest_repopath = app.repo.convert_path(&pkg.manifest_path)?;
            let (prefix, _) = manifest_repopath.split_basename();

            let qnames = vec![pkg.name.to_owned(), "cargo".to_owned()];

            if let Some(ident) = app.graph.try_add_project(qnames, pconfig) {
                let proj = app.graph.lookup_mut(ident);

                // Q: should we include a registry name as a qualifier?
                proj.version = Some(Version::Semver(pkg.version.clone()));
                proj.prefix = Some(prefix.to_owned());
                cargo_to_graph.insert(pkg.id.clone(), ident);

                // Auto-register a rewriter to update this package's Cargo.toml.
                let cargo_rewrite = CargoRewriter::new(ident, manifest_repopath);
                proj.rewriters.push(Box::new(cargo_rewrite));
            }
        }

        // Now establish the interdependencies.

        let mut cargoid_to_index = HashMap::new();

        for (index, pkg) in cargo_meta.packages[..].iter().enumerate() {
            cargoid_to_index.insert(pkg.id.clone(), index);
        }

        for node in &cargo_meta.resolve.unwrap().nodes {
            let pkg = &cargo_meta.packages[cargoid_to_index[&node.id]];

            if let Some(depender_id) = cargo_to_graph.get(&node.id) {
                let maybe_versions = pkg.metadata.get("internal_dep_versions");
                let manifest_repopath = app.repo.convert_path(&pkg.manifest_path)?;

                for dep in &node.deps {
                    if let Some(dependee_id) = cargo_to_graph.get(&dep.pkg) {
                        // Find the literal dependency info that Cargo sees. In
                        // typical cases this should be "0.0.0-dev.0" or its
                        // equivalent, but during bootstrap it might be a "real"
                        // version.
                        //
                        // XXX: Repeated linear search is lame.

                        let mut literal = None;

                        for cargo_dep in &pkg.dependencies[..] {
                            let cmp_name = cargo_dep.rename.as_ref().unwrap_or(&cargo_dep.name);

                            if cmp_name == &dep.name {
                                literal = Some(cargo_dep.req.to_string());
                                break;
                            }
                        }

                        let literal = literal.unwrap_or_else(|| {
                            // We only rarely actually use this information, so
                            // I think it's resonable to warn here and hope for
                            // the best, rather than hard-erroring out, since
                            // I'm not 100% sure that our analysis above will
                            // always be reliable.
                            warn!("cannot find Cargo version requirement for dependency of `{}` on `{}`", &pkg.name, &dep.name);
                            "UNDEFINED".to_owned()
                        });

                        // Find the Cranko-augmented dependency info.

                        let req = maybe_versions
                            .and_then(|table| table.get(&dep.name))
                            .and_then(|nameval| nameval.as_str())
                            .map(|text| app.repo.parse_history_ref(text))
                            .transpose()?
                            .map(|cref| app.repo.resolve_history_ref(&cref, &manifest_repopath))
                            .transpose()?;

                        if req.is_none() {
                            warn!(
                                "missing or invalid key `internal_dep_versions.{}` in `{}`",
                                &dep.name, pkg.manifest_path
                            );
                            warn!("... this is needed to specify the oldest version of `{}` compatible with `{}`",
                                &dep.name, &pkg.name);
                        }

                        let req = req.unwrap_or(DepRequirement::Unavailable);

                        app.graph.add_dependency(
                            *depender_id,
                            DependencyTarget::Ident(*dependee_id),
                            literal,
                            req,
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

/// Rewrite Cargo.toml to include real version numbers.
#[derive(Debug)]
pub struct CargoRewriter {
    proj_id: ProjectId,
    toml_path: RepoPathBuf,
}

impl CargoRewriter {
    /// Create a new Cargo.toml rewriter.
    pub fn new(proj_id: ProjectId, toml_path: RepoPathBuf) -> Self {
        CargoRewriter { proj_id, toml_path }
    }
}

impl Rewriter for CargoRewriter {
    fn rewrite(&self, app: &AppSession, changes: &mut ChangeList) -> Result<()> {
        // Parse the current Cargo.toml using toml_edit so we can rewrite it
        // with minimal deltas.
        let toml_path = app.repo.resolve_workdir(&self.toml_path);
        let mut s = String::new();
        {
            let mut f = File::open(&toml_path)?;
            f.read_to_string(&mut s)?;
        }
        let mut doc: Document = s.parse()?;

        // Helper table for applying internal deps. Note that we use the 0'th
        // qname, not the user-facing name, since that is what is used in
        // Cargo-land.

        let proj = app.graph().lookup(self.proj_id);
        let mut internal_reqs = HashMap::new();

        for dep in &proj.internal_deps[..] {
            let req_text = match dep.cranko_requirement {
                DepRequirement::Manual(ref t) => t.clone(),

                DepRequirement::Commit(_) => {
                    if let Some(ref v) = dep.resolved_version {
                        // Hack: For versions before 1.0, semver treats minor
                        // versions as incompatible: ^0.1 is not compatible with
                        // 0.2. This busts our paradigm. We can work around by
                        // using explicit greater-than expressions.
                        let v = v.to_string();
                        if v.starts_with("0.") {
                            format!(">={v},<1")
                        } else {
                            format!("^{v}")
                        }
                    } else {
                        continue;
                    }
                }

                DepRequirement::Unavailable => continue,
            };

            internal_reqs.insert(
                app.graph().lookup(dep.ident).qualified_names()[0].clone(),
                req_text,
            );
        }

        // Update the project version

        {
            let ct_root = doc.as_table_mut();
            let ct_package = ct_root
                .get_mut("package")
                .and_then(|i| i.as_table_mut())
                .ok_or_else(|| anyhow!("no [package] section in {}!?", self.toml_path.escaped()))?;

            ct_package["version"] = toml_edit::value(proj.version.to_string());

            // Rewrite any internal dependencies. These may be found in three
            // main tables and a nested table of potential target-specific
            // tables.

            for tblname in &["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(tbl) = ct_root.get_mut(tblname).and_then(|i| i.as_table_mut()) {
                    rewrite_deptable(&internal_reqs, tbl)?;
                }
            }

            if let Some(ct_target) = ct_root.get_mut("target").and_then(|i| i.as_table_mut()) {
                // As far as I can tell, no way to iterate over the table while mutating
                // its values?
                let target_specs = ct_target
                    .iter()
                    .map(|(k, _v)| k.to_owned())
                    .collect::<Vec<_>>();

                for target_spec in &target_specs[..] {
                    if let Some(tbl) = ct_target
                        .get_mut(target_spec)
                        .and_then(|i| i.as_table_mut())
                    {
                        rewrite_deptable(&internal_reqs, tbl)?;
                    }
                }
            }
        }

        fn rewrite_deptable(
            internal_reqs: &HashMap<String, String>,
            tbl: &mut toml_edit::Table,
        ) -> Result<()> {
            let deps = tbl.iter().map(|(k, _v)| k.to_owned()).collect::<Vec<_>>();

            for dep in &deps[..] {
                // ??? renamed internal deps? We could save rename informaion
                // from cargo-metadata when we load everything.

                if let Some(req_text) = internal_reqs.get(dep) {
                    if let Some(dep_tbl) = tbl.get_mut(dep).and_then(|i| i.as_table_mut()) {
                        dep_tbl["version"] = toml_edit::value(req_text.clone());
                    } else if let Some(dep_tbl) =
                        tbl.get_mut(dep).and_then(|i| i.as_inline_table_mut())
                    {
                        // Can't just index inline tables???
                        if let Some(val) = dep_tbl.get_mut("version") {
                            *val = req_text.clone().into();
                        } else {
                            dep_tbl.get_or_insert("version", req_text.clone());
                        }
                    } else {
                        return Err(anyhow!(
                            "unexpected internal dependency item in a Cargo.toml: {:?}",
                            tbl.get(dep)
                        ));
                    }
                }
            }

            Ok(())
        }

        // Rewrite.

        {
            let mut f = File::create(&toml_path)?;
            write!(f, "{doc}")?;
            changes.add_path(&self.toml_path);
        }

        Ok(())
    }

    /// Rewriting just the special Cranko requirement metadata.
    fn rewrite_cranko_requirements(
        &self,
        app: &AppSession,
        changes: &mut ChangeList,
    ) -> Result<()> {
        // Short-circuit if no deps. Note that we can only do this if,
        // as done below, we don't clear unexpected entries in the
        // internal_dep_versions block. Should we do that?

        if app.graph().lookup(self.proj_id).internal_deps.is_empty() {
            return Ok(());
        }

        // Load

        let toml_path = app.repo.resolve_workdir(&self.toml_path);
        let mut s = String::new();
        {
            let mut f = File::open(&toml_path)?;
            f.read_to_string(&mut s)?;
        }
        let mut doc: Document = s.parse()?;

        // Modify.

        {
            let ct_root = doc.as_table_mut();
            let ct_package = ct_root
                .get_mut("package")
                .and_then(|i| i.as_table_mut())
                .ok_or_else(|| anyhow!("no [package] section in {}?!", self.toml_path.escaped()))?;

            let tbl = ct_package
                .entry("metadata")
                .or_insert_with(|| Item::Table(Table::new()))
                .as_table_mut()
                .ok_or_else(|| {
                    anyhow!(
                        "no [package.metadata] section in {}?!",
                        self.toml_path.escaped()
                    )
                })?;

            let tbl = tbl
                .entry("internal_dep_versions")
                .or_insert_with(|| Item::Table(Table::new()))
                .as_table_mut()
                .ok_or_else(|| {
                    anyhow!(
                        "no [package.metadata.internal_dep_versions] section in {}?!",
                        self.toml_path.escaped()
                    )
                })?;

            let graph = app.graph();
            let proj = graph.lookup(self.proj_id);

            for dep in &proj.internal_deps {
                let target = &graph.lookup(dep.ident).qualified_names()[0];

                let spec = match &dep.cranko_requirement {
                    DepRequirement::Commit(cid) => cid.to_string(),
                    DepRequirement::Manual(t) => format!("manual:{t}"),
                    DepRequirement::Unavailable => continue,
                };

                tbl[target] = toml_edit::value(spec);
            }
        }

        // Rewrite.

        {
            let mut f = File::create(&toml_path)?;
            write!(f, "{doc}")?;
            changes.add_path(&self.toml_path);
        }

        Ok(())
    }
}

/// Cargo-specific CLI utilities.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub enum CargoCommands {
    #[structopt(name = "foreach-released")]
    /// Run a "cargo" command for each released Cargo project.
    ForeachReleased(ForeachReleasedCommand),

    #[structopt(name = "package-released-binaries")]
    /// Archive the executables associated with released Cargo projects.
    PackageReleasedBinaries(PackageReleasedBinariesCommand),
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct CargoCommand {
    #[structopt(subcommand)]
    command: CargoCommands,
}

impl Command for CargoCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            CargoCommands::ForeachReleased(o) => o.execute(),
            CargoCommands::PackageReleasedBinaries(o) => o.execute(),
        }
    }
}

/// `cranko cargo foreach-released`
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct ForeachReleasedCommand {
    #[structopt(
        long = "command-name",
        help = "The command name to use for Cargo",
        default_value = "cargo"
    )]
    command_name: String,

    #[structopt(
        long = "pause",
        help = "Pause a number of seconds between command invocations",
        default_value = "0"
    )]
    pause: u64,

    #[structopt(help = "Arguments to the `cargo` command", required = true)]
    cargo_args: Vec<OsString>,
}

impl Command for ForeachReleasedCommand {
    fn execute(self) -> Result<i32> {
        let sess = AppSession::initialize_default()?;

        let (dev_mode, rel_info) = sess.ensure_ci_release_mode()?;
        if dev_mode {
            warn!("proceeding even though in dev mode");
        }

        let mut q = GraphQueryBuilder::default();
        q.only_new_releases(rel_info);
        q.only_project_type("cargo");
        let idents = sess
            .graph()
            .query(q)
            .context("could not select projects for cargo foreach-released")?;

        let mut cmd = process::Command::new(&self.command_name);
        cmd.args(&self.cargo_args[..]);

        let print_which = idents.len() > 1;
        let pause_dur = time::Duration::from_secs(self.pause);
        let mut first = true;

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);
            let dir = sess.repo.resolve_workdir(proj.prefix());
            cmd.current_dir(&dir);

            if self.pause != 0 && !first {
                println!("### pausing for {} seconds", self.pause);
                thread::sleep(pause_dur);
            }

            if print_which {
                if !first {
                    println!();
                }

                println!("### in `{}`:", dir.display());
            }

            if first {
                first = false;
            }

            let status = cmd.status().context(format!(
                "could not run the cargo command for project `{}`",
                proj.user_facing_name
            ))?;
            if !status.success() {
                return Err(anyhow!(
                    "the command cargo failed for project `{}`",
                    proj.user_facing_name
                ));
            }
        }

        Ok(0)
    }
}

/// `cranko cargo package-released-binaries`
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct PackageReleasedBinariesCommand {
    #[structopt(
        long = "command-name",
        help = "The command name to use for Cargo",
        default_value = "cargo"
    )]
    command_name: String,

    #[structopt(
        long = "reroot",
        help = "A prefix to apply to paths returned by the invoked tool"
    )]
    reroot: Option<OsString>,

    #[structopt(short = "t", long = "target", help = "The binaries' target platform")]
    target: String,

    #[structopt(
        help = "The directory into which the archive files should be placed",
        required = true
    )]
    dest_dir: PathBuf,

    #[structopt(
        last(true),
        help = "Arguments to the `cargo` command used to build/detect binaries",
        required = true
    )]
    cargo_args: Vec<OsString>,
}

impl Command for PackageReleasedBinariesCommand {
    fn execute(self) -> Result<i32> {
        use cargo_metadata::Message;

        let sess = AppSession::initialize_default()?;

        // For this command, it is OK to run in dev mode
        let (_dev_mode, rel_info) = sess.ensure_ci_release_mode()?;

        let target: target_lexicon::Triple = self
            .target
            .parse()
            .map_err(|e| anyhow!("could not parse target \"triple\" `{}`: {}", self.target, e))?;
        let mode = BinaryArchiveMode::from(&target);

        let mut q = GraphQueryBuilder::default();
        q.only_new_releases(rel_info);
        q.only_project_type("cargo");
        let idents = sess.graph().query(q).context("could not select projects")?;

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);

            // Unlike foreach-released, here we iterate over projects by passing
            // a --package argument rather than spawning the process in a
            // subdirectory. This is to ensure that we work with `cross`, which
            // seemingly only looks for Cross.toml in the current directory, and
            // also tempts people into using relative paths for arguments like
            // `--reroot`.
            let mut cmd = process::Command::new(&self.command_name);
            cmd.args(&self.cargo_args[..])
                .arg("--message-format=json")
                .arg(format!("--package={}", proj.qualified_names()[0]))
                .stdout(process::Stdio::piped());

            let mut child = cmd
                .spawn()
                .with_context(|| format!("failed to spawn subcommand: {cmd:?}"))?;
            let reader = BufReader::new(child.stdout.take().unwrap());

            let mut binaries = Vec::new();

            for message in Message::parse_stream(reader) {
                match message.unwrap() {
                    Message::CompilerMessage(msg) => {
                        println!("{msg}");
                    }

                    Message::CompilerArtifact(artifact) => {
                        if let Some(p) = artifact.executable {
                            binaries.push(if let Some(ref root) = self.reroot {
                                let mut prefixed = root.clone();
                                prefixed.push(p);
                                PathBuf::from(prefixed)
                            } else {
                                p.into_std_path_buf()
                            });
                        }
                    }

                    _ => {}
                }
            }

            let status = child.wait().context("couldn't get cargo's exit status")?;
            if !status.success() {
                return Err(anyhow!(
                    "the command cargo failed for project `{}`",
                    proj.user_facing_name
                ));
            }

            if binaries.is_empty() {
                // Don't issue a warning -- we don't offer any way to filter the
                // list of projects that is considered.
                continue;
            }

            let archive_path = mode
                .archive_binaries(proj, &self.dest_dir, &binaries, &target)
                .context("couldn't create archive")?;
            info!(
                "`{}` => {} ({} files)",
                proj.user_facing_name,
                archive_path.display(),
                binaries.len(),
            );
        }

        Ok(0)
    }
}

enum BinaryArchiveMode {
    Tarball,
    Zipball,
}

impl BinaryArchiveMode {
    fn archive_binaries(
        &self,
        proj: &Project,
        dest_dir: &Path,
        binaries: &[PathBuf],
        target: &target_lexicon::Triple,
    ) -> Result<PathBuf> {
        match self {
            BinaryArchiveMode::Tarball => self.tarball(proj, dest_dir, binaries, target),
            BinaryArchiveMode::Zipball => self.zipball(proj, dest_dir, binaries, target),
        }
    }

    fn zipball(
        &self,
        proj: &Project,
        dest_dir: &Path,
        binaries: &[PathBuf],
        target: &target_lexicon::Triple,
    ) -> Result<PathBuf> {
        let mut path = dest_dir.to_path_buf();
        path.push(format!(
            "{}-{}-{}.zip",
            proj.qualified_names()[0],
            proj.version,
            target
        ));

        let out_file = File::create(&path)
            .with_context(|| format!("failed to create Zip file `{}`", path.display()))?;
        let mut zip = zip::ZipWriter::new(out_file);
        zip.set_comment("Created by Cranko");

        let options = zip::write::FileOptions::default().unix_permissions(0o755);

        for bin in binaries {
            let name = bin
                .file_name()
                .ok_or_else(|| anyhow!("cargo output binary {} is a directory??", bin.display()))?;
            let name = name.to_str().ok_or_else(|| {
                anyhow!(
                    "cargo output binary {} name is not Unicode-compatible",
                    bin.display()
                )
            })?;

            let mut in_file = File::open(bin)
                .with_context(|| format!("failed to open executable file `{}`", bin.display()))?;

            zip.start_file(name, options).with_context(|| {
                format!(
                    "could not start record for executable `{}` in Zip `{}`",
                    bin.display(),
                    path.display()
                )
            })?;
            std::io::copy(&mut in_file, &mut zip).with_context(|| {
                format!(
                    "could not copy data for executable `{}` into Zip `{}`",
                    bin.display(),
                    path.display()
                )
            })?;
        }

        zip.finish()
            .with_context(|| format!("failed to finish writing Zip file `{}`", path.display()))?;
        Ok(path)
    }

    fn tarball(
        &self,
        proj: &Project,
        dest_dir: &Path,
        binaries: &[PathBuf],
        target: &target_lexicon::Triple,
    ) -> Result<PathBuf> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let mut path = dest_dir.to_path_buf();
        path.push(format!(
            "{}-{}-{}.tar.gz",
            proj.qualified_names()[0],
            proj.version,
            target
        ));

        let file = File::create(&path)
            .with_context(|| format!("failed to create tar file `{}`", path.display()))?;
        let enc = GzEncoder::new(file, Compression::default());
        let mut tar = tar::Builder::new(enc);

        for bin in binaries {
            let name = bin
                .file_name()
                .ok_or_else(|| anyhow!("cargo output binary {} is a directory??", bin.display()))?;
            tar.append_path_with_name(bin, name)
                .with_context(|| format!("failed to add file `{}` to tar", bin.display()))?;
        }

        tar.finish()
            .with_context(|| format!("failed to finish writing tar file `{}`", path.display()))?;
        Ok(path)
    }
}

impl Default for BinaryArchiveMode {
    #[cfg(windows)]
    fn default() -> Self {
        BinaryArchiveMode::Zipball
    }

    #[cfg(not(windows))]
    fn default() -> Self {
        BinaryArchiveMode::Tarball
    }
}

impl From<&target_lexicon::Triple> for BinaryArchiveMode {
    fn from(trip: &target_lexicon::Triple) -> Self {
        match trip.operating_system {
            target_lexicon::OperatingSystem::Windows => BinaryArchiveMode::Zipball,
            _ => BinaryArchiveMode::Tarball,
        }
    }
}
