// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! NPM (JavaScripty) projects.
//!
//! In order to operate on these, we need to rewrite `package.json` files. As
//! far as I can tell, there isn't a Rust library to load and store JSON in a
//! format-preserving way, so we might generate large diffs. Would be good to
//! fix that.

use anyhow::{anyhow, Context};
use log::warn;
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs::{File, OpenOptions},
    process,
};
use structopt::StructOpt;

use super::Command;

use crate::{
    app::{AppBuilder, AppSession},
    atry,
    config::ProjectConfiguration,
    errors::Result,
    graph::{GraphQueryBuilder, ProjectGraphBuilder},
    project::{DepRequirement, DependencyTarget, ProjectId},
    repository::{ChangeList, RepoPath, RepoPathBuf, Repository},
    rewriters::Rewriter,
    version::Version,
    write_crlf,
};

const DEPENDENCY_KEYS: &[&str] = &["dependencies", "devDependencies", "optionalDependencies"];

/// Framework for auto-loading NPM projects from the repository contents.
#[derive(Debug, Default)]
pub struct NpmLoader {
    npm_to_graph: HashMap<String, PackageLoadData>,
}

#[derive(Debug)]
struct PackageLoadData {
    ident: ProjectId,
    json_path: RepoPathBuf,
    pkg_data: serde_json::Map<String, serde_json::Value>,
}

impl NpmLoader {
    // TODO: should should defer detailed processing to finalize() like the
    // othe loaders
    pub fn process_index_item(
        &mut self,
        repo: &Repository,
        graph: &mut ProjectGraphBuilder,
        repopath: &RepoPath,
        dirname: &RepoPath,
        basename: &RepoPath,
        pconfig: &HashMap<String, ProjectConfiguration>,
    ) -> Result<()> {
        if basename.as_ref() != b"package.json" {
            return Ok(());
        }

        // Parse the JSON.
        let path = repo.resolve_workdir(repopath);
        let f = atry!(
            File::open(&path);
            ["failed to open repository file `{}`", path.display()]
        );
        let pkg_data: serde_json::Map<String, serde_json::Value> = atry!(
            serde_json::from_reader(f);
            ["failed to parse file `{}` as JSON", path.display()]
        );

        // Does this package.json seem to describe an actual package with
        // content? When using Lerna, there may be a toplevel package.json that
        // specifies deps but doesn't actually contain any code itself.

        const CONTENT_KEYS: &[&str] = &["bin", "browser", "files", "main", "types", "version"];
        let has_content = CONTENT_KEYS.iter().any(|k| pkg_data.contains_key(*k));
        if !has_content {
            return Ok(());
        }

        // Load up the basic info.

        let name = pkg_data
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "NPM file `{}` does not have a string-typed `name` field",
                    path.display()
                )
            })?
            .to_owned();

        let version = pkg_data
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "NPM file `{}` does not have a string-typed `version` field",
                    path.display()
                )
            })?;
        let version = atry!(
            semver::Version::parse(version);
            ["cannot parse `version` field \"{}\" in `{}` as a semver version",
             version, path.display()]
        );

        let qnames = vec![name.to_owned(), "npm".to_owned()];

        if let Some(ident) = graph.try_add_project(qnames, pconfig) {
            let proj = graph.lookup_mut(ident);
            proj.prefix = Some(dirname.to_owned());
            proj.version = Some(Version::Semver(version));

            // Auto-register a rewriter to update this package's package.json.
            let rewrite = PackageJsonRewriter::new(ident, repopath.to_owned());
            proj.rewriters.push(Box::new(rewrite));

            // Save the info for dep-linking later.
            self.npm_to_graph.insert(
                name,
                PackageLoadData {
                    ident,
                    pkg_data,
                    json_path: repopath.to_owned(),
                },
            );
        }

        Ok(())
    }

    /// Finalize autoloading any NPM projects. Consumes this object.
    pub fn finalize(self, app: &mut AppBuilder) -> Result<()> {
        // We're just going to assume that if there are multiple packages in the
        // repo where one has a dep with a name equal to a different one, it's
        // an internal dependency. That's not too simpleminded, right?

        for (name, load_data) in &self.npm_to_graph {
            let maybe_internal_specs = load_data
                .pkg_data
                .get("internalDepVersions")
                .and_then(|v| v.as_object());

            for dep_key in DEPENDENCY_KEYS {
                if let Some(dep_map) = load_data.pkg_data.get(*dep_key).and_then(|v| v.as_object())
                {
                    for (dep_name, dep_spec) in dep_map {
                        if let Some(dep_data) = &self.npm_to_graph.get(dep_name) {
                            let req = if let Some(cranko_spec) = maybe_internal_specs
                                .and_then(|d| d.get(dep_name))
                                .and_then(|v| v.as_str())
                            {
                                match app.repo.parse_history_ref(cranko_spec).and_then(|cref| {
                                    app.repo.resolve_history_ref(&cref, &load_data.json_path)
                                }) {
                                    Ok(r) => r,

                                    Err(e) => {
                                        warn!("invalid `package.json` key `internalDepVersions.{}` for {}: {}",
                                            dep_name, name, e);
                                        DepRequirement::Unavailable
                                    }
                                }
                            } else {
                                DepRequirement::Unavailable
                            };

                            app.graph.add_dependency(
                                load_data.ident,
                                DependencyTarget::Ident(dep_data.ident),
                                dep_spec.as_str().unwrap_or("UNDEFINED").to_owned(),
                                req,
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// Rewrite `package.json` to include real version numbers.
#[derive(Debug)]
pub struct PackageJsonRewriter {
    proj_id: ProjectId,
    json_path: RepoPathBuf,
}

impl PackageJsonRewriter {
    /// Create a new `package.json` rewriter.
    pub fn new(proj_id: ProjectId, json_path: RepoPathBuf) -> Self {
        PackageJsonRewriter { proj_id, json_path }
    }
}

impl Rewriter for PackageJsonRewriter {
    fn rewrite(&self, app: &AppSession, changes: &mut ChangeList) -> Result<()> {
        let path = app.repo.resolve_workdir(&self.json_path);

        // Parse the JSON.
        let mut pkg_data: serde_json::Map<String, serde_json::Value> = {
            let f = atry!(
                File::open(&path);
                ["failed to open file `{}`", path.display()]
            );
            atry!(
                serde_json::from_reader(f);
                ["failed to parse file `{}` as JSON", path.display()]
            )
        };

        // Helper table for applying internal deps. Note that we use the 0'th
        // qname, not the user-facing name, since that is what is used in
        // NPM-land.

        let proj = app.graph().lookup(self.proj_id);
        let mut internal_reqs = HashMap::new();

        for dep in &proj.internal_deps[..] {
            let req_text = match dep.cranko_requirement {
                DepRequirement::Manual(ref t) => t.clone(),

                DepRequirement::Commit(_) => {
                    if let Some(ref v) = dep.resolved_version {
                        // The user can configure a custom resolution protocol
                        // to prepend to the version. This capability basically
                        // exists to let us prepend a `workspace:` when using
                        // Yarn workspaces, which helps ensure that we always
                        // resolve internal deps internally.

                        let (protocol, sep) = app
                            .npm_config
                            .internal_dep_protocol
                            .as_ref()
                            .map(|p| (p.as_ref(), ":"))
                            .unwrap_or(("", ""));

                        // Hack: For versions before 1.0, semver treats minor
                        // versions as incompatible: ^0.1 is not compatible with
                        // 0.2. This busts our paradigm. We can work around by
                        // using explicit greater-than expressions, but
                        // unfortunately in Yarn workspace expressions it seems
                        // that we can't add an upper "<1" constraint too.
                        let v = v.to_string();
                        if v.starts_with("0.") {
                            format!("{protocol}{sep}>={v}")
                        } else {
                            format!("{protocol}{sep}^{v}")
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

        // Update everything.

        pkg_data["version"] = serde_json::Value::String(proj.version.to_string());

        for dep_key in DEPENDENCY_KEYS {
            if let Some(dep_map) = pkg_data.get_mut(*dep_key).and_then(|v| v.as_object_mut()) {
                for (dep_name, dep_spec) in dep_map.iter_mut() {
                    if let Some(text) = internal_reqs.get(dep_name) {
                        *dep_spec = serde_json::Value::String(text.clone());
                    }
                }
            }
        }

        // Write it out again.

        {
            let mut f = File::create(&path)?;
            atry!(
                serde_json::to_writer_pretty(&mut f, &pkg_data);
                ["failed to overwrite JSON file `{}`", path.display()]
            );
            atry!(
                write_crlf!(f, "");
                ["failed to overwrite JSON file `{}`", path.display()]
            );
            changes.add_path(&self.json_path);
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

        // Parse the JSON.

        let path = app.repo.resolve_workdir(&self.json_path);

        let mut pkg_data: serde_json::Map<String, serde_json::Value> = {
            let f = atry!(
                File::open(&path);
                ["failed to open file `{}`", path.display()]
            );
            atry!(
                serde_json::from_reader(f);
                ["failed to parse file `{}` as JSON", path.display()]
            )
        };

        // Mutate.

        let reqs = match pkg_data
            .get_mut("internalDepVersions")
            .and_then(|v| v.as_object_mut())
        {
            Some(t) => t,

            None => {
                pkg_data.insert(
                    "internalDepVersions".to_owned(),
                    serde_json::Value::Object(serde_json::Map::new()),
                );
                pkg_data["internalDepVersions"].as_object_mut().unwrap()
            }
        };

        let graph = app.graph();
        let proj = graph.lookup(self.proj_id);

        for dep in &proj.internal_deps {
            let target = &graph.lookup(dep.ident).qualified_names()[0];

            let spec = match &dep.cranko_requirement {
                DepRequirement::Commit(cid) => cid.to_string(),
                DepRequirement::Manual(t) => format!("manual:{t}"),
                DepRequirement::Unavailable => continue,
            };

            reqs.insert(target.to_owned(), serde_json::Value::String(spec));
        }

        // Write it out again.

        {
            let f = File::create(&path)?;
            atry!(
                serde_json::to_writer_pretty(f, &pkg_data);
                ["failed to overwrite JSON file `{}`", path.display()]
            );
            changes.add_path(&self.json_path);
        }

        Ok(())
    }
}

/// Npm-specific CLI utilities.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub enum NpmCommands {
    #[structopt(name = "foreach-released")]
    /// Run a command for each released NPM project.
    ForeachReleased(ForeachReleasedCommand),

    #[structopt(name = "install-token")]
    /// Install $NPM_TOKEN in the user's .npmrc or .yarnrc.yml
    InstallToken(InstallTokenCommand),

    #[structopt(name = "lerna-workaround")]
    /// Write incorrect internal version requirements so that Lerna can
    /// understand them.
    LernaWorkaround(LernaWorkaroundCommand),
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct NpmCommand {
    #[structopt(subcommand)]
    command: NpmCommands,
}

impl Command for NpmCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            NpmCommands::ForeachReleased(o) => o.execute(),
            NpmCommands::InstallToken(o) => o.execute(),
            NpmCommands::LernaWorkaround(o) => o.execute(),
        }
    }
}

/// `cranko npm foreach-released`
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct ForeachReleasedCommand {
    #[structopt(help = "The command to run", required = true)]
    command: Vec<OsString>,
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
        q.only_project_type("npm");
        let idents = sess
            .graph()
            .query(q)
            .context("could not select projects for `npm foreach-released`")?;

        let mut cmd = process::Command::new(&self.command[0]);
        if self.command.len() > 1 {
            cmd.args(&self.command[1..]);
        }

        let print_which = idents.len() > 1;
        let mut first = true;

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);
            let dir = sess.repo.resolve_workdir(proj.prefix());
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
                "could not run the command for NPM project `{}`",
                proj.user_facing_name
            ))?;
            if !status.success() {
                return Err(anyhow!(
                    "the command failed for NPM project `{}`",
                    proj.user_facing_name
                ));
            }
        }

        Ok(0)
    }
}

/// `cranko npm install-token`
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct InstallTokenCommand {
    #[structopt(long)]
    yarn: bool,

    #[structopt(long = "registry", help = "The registry base URL.")]
    registry: Option<String>,
}

impl Command for InstallTokenCommand {
    fn execute(self) -> Result<i32> {
        let token = atry!(
            env::var("NPM_TOKEN");
            ["missing or non-textual environment variable NPM_TOKEN"]
        );

        let mut p =
            dirs::home_dir().ok_or_else(|| anyhow!("cannot determine user's home directory"))?;

        if self.yarn {
            let registry = self
                .registry
                .unwrap_or_else(|| "https://registry.yarnpkg.com/".to_owned());

            p.push(".yarnrc.yml");

            let mut file = atry!(
                OpenOptions::new().create(true).append(true).open(&p);
                ["failed to open file `{}` for appending", p.display()]
            );

            atry!(
                write_crlf!(file, "npmRegistries:");
                ["failed to write token data to file `{}`", p.display()]
            );
            atry!(
                write_crlf!(file, "  \"{}\":", registry);
                ["failed to write token data to file `{}`", p.display()]
            );
            atry!(
                write_crlf!(file, "    npmAuthToken: {}", token);
                ["failed to write token data to file `{}`", p.display()]
            );
        } else {
            let registry = self
                .registry
                .unwrap_or_else(|| "//registry.npmjs.org/".to_owned());

            p.push(".npmrc");

            let mut file = atry!(
                OpenOptions::new().create(true).append(true).open(&p);
                ["failed to open file `{}` for appending", p.display()]
            );

            atry!(
                write_crlf!(file, "{}:_authToken={}", registry, token);
                ["failed to write token data to file `{}`", p.display()]
            );
        }

        Ok(0)
    }
}

/// `cranko npm lerna-workaround`
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct LernaWorkaroundCommand {}

impl Command for LernaWorkaroundCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = AppSession::initialize_default()?;

        let mut q = GraphQueryBuilder::default();
        q.only_project_type("npm");
        let idents = sess
            .graph()
            .query(q)
            .context("could not select projects for `npm lerna-workaround`")?;

        sess.fake_internal_deps();

        let mut changes = ChangeList::default();

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);

            for rw in &proj.rewriters {
                atry!(
                    rw.rewrite(&sess, &mut changes);
                    ["failed to rewrite metadata for `{}`", proj.user_facing_name]
                );
            }
        }

        Ok(0)
    }
}
