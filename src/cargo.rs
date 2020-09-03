// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
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
    process,
};
use structopt::StructOpt;
use toml_edit::Document;

use super::Command;

use crate::{
    app::AppSession,
    errors::{Error, Result},
    graph::GraphQueryBuilder,
    project::{Project, ProjectId},
    repository::{ChangeList, RepoPath, RepoPathBuf},
    rewriters::Rewriter,
    version::Version,
};

/// Framework for auto-loading Cargo projects from the repository contents.
#[derive(Debug)]
pub struct CargoLoader {
    shortest_toml_dirname: Option<RepoPathBuf>,
}

impl Default for CargoLoader {
    fn default() -> Self {
        CargoLoader {
            shortest_toml_dirname: None,
        }
    }
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
    pub fn finalize(self, app: &mut AppSession) -> Result<()> {
        let shortest_toml_dirname = match self.shortest_toml_dirname {
            Some(d) => d,
            None => return Ok(()),
        };

        let mut toml_path = app.repo.resolve_workdir(&shortest_toml_dirname);
        toml_path.push("Cargo.toml");
        let mut cmd = MetadataCommand::new();
        cmd.manifest_path(&toml_path);
        cmd.features(cargo_metadata::CargoOpt::AllFeatures);
        let cargo_meta = cmd.exec()?;

        // Fill in the packages

        let mut cargo_to_graph = HashMap::new();

        for pkg in &cargo_meta.packages {
            if pkg.source.is_some() {
                continue; // This is an external package; not to be tracked.
            }

            // Auto-register a rewriter to update this package's Cargo.toml.
            let manifest_repopath = app.repo.convert_path(&pkg.manifest_path)?;
            let (prefix, _) = manifest_repopath.split_basename();

            let mut pb = app.graph_mut().add_project();

            // Q: should we include a registry name as a qualifier?
            pb.qnames(&[&pkg.name, "cargo"])
                .version(Version::Semver(pkg.version.clone()));
            pb.prefix(prefix.to_owned());

            let ident = pb.finish_init();
            cargo_to_graph.insert(pkg.id.clone(), ident);

            // Auto-register a rewriter to update this package's Cargo.toml.
            let cargo_rewrite = CargoRewriter::new(ident, manifest_repopath);
            app.graph_mut()
                .lookup_mut(ident)
                .rewriters
                .push(Box::new(cargo_rewrite));
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
                        let min_version = maybe_versions
                            .and_then(|table| table.get(&dep.name))
                            .and_then(|nameval| nameval.as_str())
                            .map(|text| app.repo.parse_commit_ref(text))
                            .transpose()?
                            .map(|cref| app.repo.resolve_commit_ref(&cref, &manifest_repopath))
                            .transpose()?;

                        if min_version.is_none() {
                            warn!(
                                "missing or invalid key `internal_dep_versions.{}` in `{}`",
                                &dep.name,
                                pkg.manifest_path.display()
                            );
                            warn!("... this is needed to specify the oldest version of `{}` compatible with `{}`",
                                &dep.name, &pkg.name);
                        }

                        app.graph_mut()
                            .add_dependency(*depender_id, *dependee_id, min_version);
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

        for req in &proj.internal_reqs[..] {
            internal_reqs.insert(
                app.graph().lookup(req.ident).qualified_names()[0].clone(),
                req.min_version.clone(),
            );
        }

        // Update the project version

        {
            let ct_root = doc.as_table_mut();
            let ct_package = ct_root.entry("package").as_table_mut().ok_or_else(|| {
                Error::RewriteFormatError(format!(
                    "no [package] section in {}?!",
                    self.toml_path.escaped()
                ))
            })?;

            ct_package["version"] = toml_edit::value(proj.version.to_string());

            // Rewrite any internal dependencies. These may be found in three
            // main tables and a nested table of potential target-specific
            // tables.

            for tblname in &["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(tbl) = ct_root.entry(tblname).as_table_mut() {
                    rewrite_deptable(&internal_reqs, tbl)?;
                }
            }

            if let Some(ct_target) = ct_root.entry("target").as_table_mut() {
                // As far as I can tell, no way to iterate over the table while mutating
                // its values?
                let target_specs = ct_target
                    .iter()
                    .map(|(k, _v)| k.to_owned())
                    .collect::<Vec<_>>();

                for target_spec in &target_specs[..] {
                    if let Some(tbl) = ct_target.entry(target_spec).as_table_mut() {
                        rewrite_deptable(&internal_reqs, tbl)?;
                    }
                }
            }
        }

        fn rewrite_deptable(
            internal_reqs: &HashMap<String, Version>,
            tbl: &mut toml_edit::Table,
        ) -> Result<()> {
            let deps = tbl.iter().map(|(k, _v)| k.to_owned()).collect::<Vec<_>>();

            for dep in &deps[..] {
                // ??? renamed internal deps? We could save rename informaion
                // from cargo-metadata when we load everything.

                if let Some(min_version) = internal_reqs.get(dep) {
                    if let Some(dep_tbl) = tbl.entry(dep).as_table_mut() {
                        dep_tbl["version"] = toml_edit::value(format!("^{}", min_version));
                    } else if let Some(dep_tbl) = tbl.entry(dep).as_inline_table_mut() {
                        // Can't just index inline tables???
                        if let Some(val) = dep_tbl.get_mut("version") {
                            *val = format!("^{}", min_version).into();
                        } else {
                            dep_tbl.get_or_insert("version", format!("^{}", min_version));
                        }
                    } else {
                        return Err(Error::Environment(format!(
                            "unexpected internal dependency item in a Cargo.toml: {:?}",
                            tbl.entry(dep)
                        )));
                    }
                }
            }

            Ok(())
        }

        // Rewrite.

        {
            let mut f = File::create(&toml_path)?;
            write!(f, "{}", doc.to_string_in_original_order())?;
            changes.add_path(&self.toml_path);
        }

        Ok(())
    }
}

/// Cargo-specific CLI utilities.
#[derive(Debug, PartialEq, StructOpt)]
pub enum CargoCommands {
    #[structopt(name = "foreach-released")]
    /// Run a "cargo" command for each released Cargo project.
    ForeachReleased(ForeachReleasedCommand),

    #[structopt(name = "package-released-binaries")]
    /// Archive the executables associated with released Cargo projects.
    PackageReleasedBinaries(PackageReleasedBinariesCommand),
}

#[derive(Debug, PartialEq, StructOpt)]
pub struct CargoCommand {
    #[structopt(subcommand)]
    command: CargoCommands,
}

impl Command for CargoCommand {
    fn execute(self) -> anyhow::Result<i32> {
        match self.command {
            CargoCommands::ForeachReleased(o) => o.execute(),
            CargoCommands::PackageReleasedBinaries(o) => o.execute(),
        }
    }
}

/// `cranko cargo foreach-released`
#[derive(Debug, PartialEq, StructOpt)]
pub struct ForeachReleasedCommand {
    #[structopt(
        long = "command-name",
        help = "The command name to use for Cargo",
        default_value = "cargo"
    )]
    command_name: String,

    #[structopt(help = "Arguments to the `cargo` command", required = true)]
    cargo_args: Vec<OsString>,
}

impl Command for ForeachReleasedCommand {
    fn execute(self) -> anyhow::Result<i32> {
        let mut sess = AppSession::initialize()?;
        sess.populated_graph()?;

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
        let mut first = true;

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);
            let dir = sess.repo.resolve_workdir(&proj.prefix());
            cmd.current_dir(&dir);

            if print_which {
                if first {
                    first = false;
                } else {
                    println!();
                }
                println!("### in `{}`:", dir.display());
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
#[derive(Debug, PartialEq, StructOpt)]
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
    fn execute(self) -> anyhow::Result<i32> {
        use cargo_metadata::Message;

        let mut sess = AppSession::initialize()?;
        sess.populated_graph()?;

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

        let mut cmd = process::Command::new(&self.command_name);
        cmd.args(&self.cargo_args[..])
            .arg("--message-format=json")
            .stdout(process::Stdio::piped());

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);
            let dir = sess.repo.resolve_workdir(&proj.prefix());
            cmd.current_dir(&dir);

            let mut child = cmd.spawn()?;
            let reader = BufReader::new(child.stdout.take().unwrap());

            let mut binaries = Vec::new();

            for message in Message::parse_stream(reader) {
                match message.unwrap() {
                    Message::CompilerMessage(msg) => {
                        println!("{}", msg);
                    }

                    Message::CompilerArtifact(artifact) => {
                        if let Some(p) = artifact.executable {
                            binaries.push(if let Some(ref root) = self.reroot {
                                let mut prefixed = root.clone();
                                prefixed.push(p);
                                PathBuf::from(prefixed)
                            } else {
                                p
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
            proj.version.to_string(),
            target
        ));

        let out_file = File::create(&path)?;
        let mut zip = zip::ZipWriter::new(out_file);
        zip.set_comment("Created by Cranko");

        let options = zip::write::FileOptions::default().unix_permissions(0o755);

        for bin in binaries {
            let name = bin.file_name().ok_or_else(|| {
                Error::Environment(format!(
                    "cargo output binary {} is a directory??",
                    bin.display()
                ))
            })?;
            let name = name.to_str().ok_or_else(|| {
                Error::Environment(format!(
                    "cargo output binary {} name is not Unicode-compatible",
                    bin.display()
                ))
            })?;

            let mut in_file = File::open(bin)?;

            zip.start_file(name, options)
                .map_err(|e| Error::Environment(format!("could not start Zip entry: {}", e)))?;
            std::io::copy(&mut in_file, &mut zip)?;
        }

        zip.finish()
            .map_err(|e| Error::Environment(format!("could not finalize Zip file: {}", e)))?;
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
            proj.version.to_string(),
            target
        ));

        let file = File::create(&path)?;
        let enc = GzEncoder::new(file, Compression::default());
        let mut tar = tar::Builder::new(enc);

        for bin in binaries {
            let name = bin.file_name().ok_or_else(|| {
                Error::Environment(format!(
                    "cargo output binary {} is a directory??",
                    bin.display()
                ))
            })?;
            tar.append_path_with_name(bin, name)?;
        }

        tar.finish()?;
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
