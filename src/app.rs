// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State for the Cranko CLI application.

use anyhow::{anyhow, Context};
use log::{error, info, warn};
use std::collections::HashMap;
use thiserror::Error as ThisError;

use crate::{
    atry,
    config::{ConfigurationFile, NpmConfiguration},
    errors::Result,
    graph::{ProjectGraph, ProjectGraphBuilder, RepoHistories},
    project::{DepRequirement, ProjectId},
    repository::{
        ChangeList, CommitId, PathMatcher, RcCommitInfo, RcProjectInfo, ReleaseAvailability,
        ReleaseCommitInfo, Repository,
    },
    version::Version,
};

/// Setting up a Cranko application session.
pub struct AppBuilder {
    pub repo: Repository,
    pub graph: ProjectGraphBuilder,

    ci_info: ci_info::types::CiInfo,
    populate_graph: bool,
}

impl AppBuilder {
    /// Start initializing an application session.
    ///
    /// This first phase of initialization may fail if the environment doesn't
    /// associate the process with a proper Git repository with a work tree.
    pub fn new() -> Result<AppBuilder> {
        let repo = Repository::open_from_env()?;
        let graph = ProjectGraphBuilder::new();
        let ci_info = ci_info::get();

        Ok(AppBuilder {
            graph,
            repo,
            ci_info,
            populate_graph: true,
        })
    }

    pub fn populate_graph(mut self, do_populate: bool) -> Self {
        self.populate_graph = do_populate;
        self
    }

    /// Finish app initialization, yielding a full AppSession object.
    pub fn initialize(mut self) -> Result<AppSession> {
        // Start by loading the configuration file, if it exists. If it doesn't
        // we'll get a sensible default.

        let mut cfg_path = self.repo.resolve_config_dir();
        cfg_path.push("config.toml");
        let config = ConfigurationFile::get(&cfg_path).with_context(|| {
            format!(
                "failed to load repository config file `{}`",
                cfg_path.display()
            )
        })?;

        self.repo
            .apply_config(config.repo)
            .with_context(|| "failed to finalize repository setup")?;

        let proj_config = config.projects;

        // Now auto-detect everything in the repo index.

        if self.populate_graph {
            let mut cargo = crate::cargo::CargoLoader::default();
            let mut csproj = crate::csproj::CsProjLoader::default();
            let mut npm = crate::npm::NpmLoader::default();
            let mut pypa = crate::pypa::PypaLoader::default();

            // Dumb hack around the borrowchecker to allow mutable reference to
            // the graph while iterating over the repo:
            let repo = self.repo;
            let mut graph = self.graph;

            repo.scan_paths(|p| {
                let (dirname, basename) = p.split_basename();
                cargo.process_index_item(dirname, basename);
                csproj.process_index_item(&repo, p, dirname, basename)?;
                npm.process_index_item(&repo, &mut graph, p, dirname, basename, &proj_config)?;
                pypa.process_index_item(dirname, basename);
                Ok(())
            })?;

            self.repo = repo;
            self.graph = graph;
            // End dumb hack.

            cargo.finalize(&mut self, &proj_config)?;
            csproj.finalize(&mut self, &proj_config)?;
            npm.finalize(&mut self)?;
            pypa.finalize(&mut self, &proj_config)?;
        }

        // Apply project config and compile the graph.

        let graph = atry!(
            self.graph.complete_loading();
            ["the project graph is invalid"]
        );

        // All done.
        Ok(AppSession {
            repo: self.repo,
            graph,
            npm_config: config.npm,
            ci_info: self.ci_info,
        })
    }
}

/// An error returned when one project in the repository needs a newer release
/// of another project. The inner values are the user-facing names of the two
/// projects: the first named project depends on the second one.
#[derive(Debug, ThisError)]
#[error("unsatisfied internal requirement: `{0}` needs newer `{1}`")]
pub struct UnsatisfiedInternalRequirementError(pub String, pub String);

/// The main Cranko CLI application state structure.
pub struct AppSession {
    /// The backing repository.
    pub repo: Repository,

    /// It feels hacky to have this here, but this is where we need it.
    pub npm_config: NpmConfiguration,

    /// The graph of projects contained within the repo.
    graph: ProjectGraph,

    /// Information about the CI environment that we may be running in.
    ci_info: ci_info::types::CiInfo,
}

impl AppSession {
    /// Create a new app session with totally default parameters
    pub fn initialize_default() -> Result<Self> {
        AppBuilder::new()?.initialize()
    }

    /// Characterize the repository environment in which this process is
    /// running.
    pub fn execution_environment(&self) -> Result<ExecutionEnvironment> {
        if !self.ci_info.ci {
            Ok(ExecutionEnvironment::NotCi)
        } else {
            let maybe_pr = self.ci_info.pr;
            let maybe_ci_branch = self.ci_info.branch_name.as_ref().map(|s| s.as_ref());
            let rc_name = self.repo.upstream_rc_name();
            let release_name = self.repo.upstream_release_name();

            if maybe_ci_branch.is_none() {
                warn!("cannot determine the triggering branch name in this CI environment");
                warn!("... this will affect many workflow safety checks")
            }

            if let Some(true) = maybe_pr {
                if maybe_ci_branch == Some(rc_name) {
                    warn!("cranko seems to be running in a pull request to the `{}` branch; this is not recommended", rc_name);
                    warn!("... treating as a non-CI environment for safety");
                    return Ok(ExecutionEnvironment::NotCi);
                }

                if maybe_ci_branch == Some(release_name) {
                    warn!("cranko seems to be running in a pull request to the `{}` branch; this is not recommended", release_name);
                    warn!("... treating as a non-CI environment for safety");
                    return Ok(ExecutionEnvironment::NotCi);
                }
            }

            if maybe_ci_branch == Some(release_name) {
                warn!("cranko seems to be running in an update to the `{}` branch; this is not recommended", release_name);
                warn!("... treating as a non-CI environment for safety");
                return Ok(ExecutionEnvironment::NotCi);
            }

            // Gather some useful parameters ... Note: on Azure Pipelines, the
            // initial checkout is in detached-HEAD state, so on pushes to the
            // `rc` branch we can't determine `current_branch`. It would be kind
            // of tedious to force all Azure users to manually check out the RC
            // branch, so if we can parse out the RC info, let's assume that's
            // what's going on.

            let is_rc_update = maybe_ci_branch == Some(rc_name);
            let current_is_release = self
                .repo
                .current_branch_name()?
                .as_ref()
                .map(|s| s.as_ref())
                == Some(release_name);

            // If the current branch is called `release`, we insist that we can
            // parse release info from HEAD. We must be in dev mode (due to PR,
            // or dev branch update) unless we have been triggered by an update to
            // the `rc` branch.

            if current_is_release {
                let rel_info = self.repo.parse_release_info_from_head()?;
                let dev_mode = !is_rc_update;
                return Ok(ExecutionEnvironment::CiReleaseMode(dev_mode, rel_info));
            }

            // Otherwise, we must be in RC mode. If we're an update to the `rc`
            // branch, we are *not* in dev mode and we insist that we can parse
            // actual RC info from HEAD. Otherwise, we are in dev mode and we
            // fake an RC request for all projects.

            let (dev_mode, rc_info) = if is_rc_update {
                (false, self.repo.parse_rc_info_from_head()?)
            } else {
                (true, self.default_dev_rc_info())
            };

            Ok(ExecutionEnvironment::CiRcMode(dev_mode, rc_info))
        }
    }

    /// Check that the current process is running *outside* of a CI environment.
    pub fn ensure_not_ci(&self, force: bool) -> Result<()> {
        match self.execution_environment()? {
            ExecutionEnvironment::NotCi => Ok(()),

            _ => {
                warn!("CI environment detected; this is unexpected for this command");
                if force {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "refusing to proceed (use \"force\" mode to override)",
                    ))
                }
            }
        }
    }

    /// Check that the current process is running in the "release mode" CI
    /// environment, returning the latest release information. Any other
    /// circumstance results in an error.
    ///
    /// The returned boolean is true if in a "development"-like mode, false if
    /// in the intended `rc` mode.
    pub fn ensure_ci_release_mode(&self) -> Result<(bool, ReleaseCommitInfo)> {
        match self.execution_environment()? {
            ExecutionEnvironment::NotCi => {
                error!("no CI environment detected; this is unexpected for this command");
                Err(anyhow!(
                    "don't know how to obtain release information -- cannot proceed",
                ))
            }

            ExecutionEnvironment::CiReleaseMode(dev, ri) => Ok((dev, ri)),

            _ => {
                error!("unexpected CI environment detected");
                error!("... this command should only be run after switching to a local `release`-type branch");
                Err(anyhow!(
                    "don't know how to obtain release information -- cannot proceed",
                ))
            }
        }
    }

    /// Check that the current process is running and "RC"-like CI mode.
    ///
    /// The returned boolean is true if in a "development"-like mode, false if
    /// in the intended `rc` mode.
    pub fn ensure_ci_rc_mode(&self, force: bool) -> Result<(bool, RcCommitInfo)> {
        match self.execution_environment()? {
            ExecutionEnvironment::CiRcMode(dev, rci) => Ok((dev, rci)),

            ExecutionEnvironment::NotCi => {
                warn!("no CI environment detected; this is unexpected for this command");
                if force {
                    Ok((true, self.default_dev_rc_info()))
                } else {
                    Err(anyhow!(
                        "refusing to proceed (use \"force\" mode to override)",
                    ))
                }
            }

            _ => {
                warn!("unexpected CI environment detected");
                warn!("... this command should only be run in `rc` contexts");
                if force {
                    Ok((true, self.default_dev_rc_info()))
                } else {
                    Err(anyhow!(
                        "refusing to proceed (use \"force\" mode to override)",
                    ))
                }
            }
        }
    }

    /// Check that the working tree is completely clean. We allow untracked and
    /// ignored files but otherwise don't want any modifications, etc. Returns
    /// Ok if clean, an Err downcastable to DirtyRepositoryError if not. The
    /// error may have a different cause if, e.g., there is an I/O failure.
    pub fn ensure_fully_clean(&self) -> Result<()> {
        use crate::repository::DirtyRepositoryError;

        if let Some(changed_path) = self.repo.check_if_dirty(&[])? {
            Err(DirtyRepositoryError(changed_path).into())
        } else {
            Ok(())
        }
    }

    /// Check that the working tree is clean, excepting modifications to any
    /// files interpreted as changelogs. Returns Ok if clean, an Err
    /// downcastable to DirtyRepositoryError if not. The error may have a
    /// different cause if, e.g., there is an I/O failure.
    pub fn ensure_changelog_clean(&self) -> Result<()> {
        use crate::repository::DirtyRepositoryError;

        let mut matchers: Vec<Result<PathMatcher>> = self
            .graph
            .projects()
            .map(|p| p.changelog.create_path_matcher(p))
            .collect();
        let matchers: Result<Vec<PathMatcher>> = matchers.drain(..).collect();
        let matchers = matchers?;

        if let Some(changed_path) = self.repo.check_if_dirty(&matchers[..])? {
            Err(DirtyRepositoryError(changed_path).into())
        } else {
            Ok(())
        }
    }

    /// Get the graph of projects inside this app session.
    pub fn graph(&self) -> &ProjectGraph {
        &self.graph
    }

    /// Get the graph of projects inside this app session, mutably.
    pub fn graph_mut(&mut self) -> &mut ProjectGraph {
        &mut self.graph
    }

    /// Walk the project graph and solve internal dependencies.
    ///
    /// This method walks the graph in topologically-sorted order. For each
    /// project, the callback `process` is called, which should return true if a
    /// new release of the project is being scheduled. By the time the callback
    /// is called, the project's internal dependency information will have been
    /// updated: for DepRequirement::Commit deps, `resolved_version` will be a
    /// Some value containing the required version. It is possible that this
    /// version will be being released "right now".
    ///
    /// By the time the callback returns, the project's `version` field should
    /// have been updated with its reference version for this release process --
    /// which should be a new value, if the callback returns true.
    ///
    /// After processing all projects, the function will return an error if
    /// there are unsatisfiable internal dependencies. This can happen either
    /// because no sufficiently new release of the dependee exists (and it's not
    /// being released now), or the internal version requirement information
    /// hasn't been annotated.
    pub fn solve_internal_deps<F>(&mut self, mut process: F) -> Result<()>
    where
        F: FnMut(&mut Repository, &mut ProjectGraph, ProjectId) -> Result<bool>,
    {
        let mut new_versions: HashMap<ProjectId, Version> = HashMap::new();
        let toposorted_idents: Vec<_> = self.graph.toposorted().collect();
        let mut unsatisfied_deps = Vec::new();

        for ident in (toposorted_idents[..]).iter().copied() {
            // We can't conveniently navigate the deps while holding a mutable
            // ref to depending project, so do some lifetime futzing and buffer
            // up modifications to its dep info.

            unsatisfied_deps.clear();

            let mut resolved_versions = {
                let proj = self.graph.lookup(ident);
                let mut resolved_versions = Vec::new();

                for (idx, dep) in proj.internal_deps.iter().enumerate() {
                    match dep.cranko_requirement {
                        // If the requirement is of a specific commit, we need
                        // to resolve its corresponding release and/or make sure
                        // that the dependee project is also being released in
                        // this batch.
                        DepRequirement::Commit(ref cid) => {
                            let dependee_proj = self.graph.lookup(dep.ident);
                            let avail = self
                                .repo
                                .find_earliest_release_containing(dependee_proj, cid)?;

                            let resolved = match avail {
                                ReleaseAvailability::NotAvailable => {
                                    unsatisfied_deps
                                        .push(dependee_proj.user_facing_name.to_string());
                                    dependee_proj.version.clone()
                                }

                                ReleaseAvailability::ExistingRelease(ref v) => v.clone(),

                                ReleaseAvailability::NewRelease => {
                                    if let Some(v) = new_versions.get(&dep.ident) {
                                        v.clone()
                                    } else {
                                        unsatisfied_deps
                                            .push(dependee_proj.user_facing_name.to_string());
                                        dependee_proj.version.clone()
                                    }
                                }
                            };

                            resolved_versions.push((idx, resolved));
                        }

                        DepRequirement::Manual(_) => {}

                        DepRequirement::Unavailable => {
                            let dependee_proj = self.graph.lookup(dep.ident);
                            unsatisfied_deps.push(dependee_proj.user_facing_name.to_string());
                            resolved_versions.push((idx, dependee_proj.version.clone()));
                        }
                    }
                }

                resolved_versions
            };

            {
                let proj = self.graph.lookup_mut(ident);

                for (idx, resolved) in resolved_versions.drain(..) {
                    proj.internal_deps[idx].resolved_version = Some(resolved);
                }
            }

            // Now, let the callback do its thing with the project, and tell us
            // if it gets a new release.

            let updated_version = atry!(
                process(&mut self.repo, &mut self.graph, ident);
                ["failed to solve internal dependencies of project `{}`", self.graph.lookup(ident).user_facing_name]
            );

            let proj = self.graph.lookup(ident);

            if updated_version {
                if !unsatisfied_deps.is_empty() {
                    return Err(UnsatisfiedInternalRequirementError(
                        proj.user_facing_name.to_string(),
                        unsatisfied_deps.join(", "),
                    )
                    .into());
                }

                new_versions.insert(ident, proj.version.clone());
            } else if !unsatisfied_deps.is_empty() {
                warn!(
                    "project `{}` has internal requirements that won't be satisfiable in the wild, \
                     but that's OK since it's not going to be released",
                    proj.user_facing_name
                );
            }
        }

        Ok(())
    }

    /// A fake version of `solve_internal_deps`. Rather than properly expressing
    /// internal version requirements, this manually assigns each internal
    /// dependency to match exactly the version of the depended-upon package.
    /// This functionality is needed for Lerna, which otherwise isn't clever
    /// enough to correctly detect the internal dependency.
    pub fn fake_internal_deps(&mut self) {
        let toposorted_idents: Vec<_> = self.graph.toposorted().collect();

        for ident in (toposorted_idents[..]).iter().copied() {
            let mut resolved_versions = {
                let proj = self.graph.lookup(ident);
                let mut resolved_versions = Vec::new();

                for (idx, dep) in proj.internal_deps.iter().enumerate() {
                    let dependee_proj = self.graph.lookup(dep.ident);
                    resolved_versions.push((idx, dependee_proj.version.clone()));
                }

                resolved_versions
            };

            {
                let proj = self.graph.lookup_mut(ident);

                for (idx, resolved) in resolved_versions.drain(..) {
                    proj.internal_deps[idx].cranko_requirement =
                        DepRequirement::Manual(resolved.to_string());
                    proj.internal_deps[idx].resolved_version = Some(resolved);
                }
            }
        }
    }

    /// Apply version numbers given the current repository state and bump
    /// specifications.
    ///
    /// This also involves solving the version requirements for internal
    /// dependencies. If an internal dependency is unsatisfiable, the returned
    /// error will be downcastable to an UnsatisfiedInternalRequirementError.
    pub fn apply_versions(&mut self, rc_info: &RcCommitInfo) -> Result<()> {
        let latest_info = self.repo.get_latest_release_info()?;

        self.solve_internal_deps(|_repo, graph, ident| {
            let proj = graph.lookup_mut(ident);

            // Set the baseline version to the last release.

            let latest_release = latest_info.lookup_project(proj);

            proj.version = if let Some(info) = latest_release {
                proj.version.parse_like(&info.version)?
            } else {
                proj.version.zero_like()
            };

            let baseline_version = proj.version.clone();

            // If there's a bump, apply it.

            Ok(if let Some(rc) = rc_info.lookup_project(proj) {
                let scheme = proj.version.parse_bump_scheme(&rc.bump_spec)?;
                scheme.apply(&mut proj.version)?;
                info!(
                    "{}: {} => {}",
                    proj.user_facing_name, baseline_version, proj.version
                );
                true
            } else {
                info!(
                    "{}: unchanged from {}",
                    proj.user_facing_name, baseline_version
                );
                false
            })
        })
        .with_context(|| "failed to solve internal dependencies")?;

        Ok(())
    }

    /// Rewrite everyone's metadata to match our internal state.
    pub fn rewrite(&self) -> Result<ChangeList> {
        let mut changes = ChangeList::default();

        for ident in self.graph.toposorted() {
            let proj = self.graph.lookup(ident);

            for rw in &proj.rewriters {
                rw.rewrite(self, &mut changes)?;
            }
        }

        Ok(changes)
    }

    /// Like rewrite(), but only for the special Cranko requirements metadata.
    /// This is convenience functionality not needed for the main workflows.
    pub fn rewrite_cranko_requirements(&self) -> Result<ChangeList> {
        let mut changes = ChangeList::default();

        for ident in self.graph.toposorted() {
            let proj = self.graph.lookup(ident);

            for rw in &proj.rewriters {
                rw.rewrite_cranko_requirements(self, &mut changes)?;
            }
        }

        Ok(changes)
    }

    pub fn make_release_commit(&mut self, rci: &RcCommitInfo) -> Result<()> {
        self.repo.make_release_commit(&self.graph, rci)
    }

    pub fn make_rc_commit(
        &mut self,
        rcinfo: Vec<RcProjectInfo>,
        changes: &ChangeList,
    ) -> Result<()> {
        self.repo.make_rc_commit(rcinfo, changes)?;
        Ok(())
    }

    pub fn analyze_histories(&self) -> Result<RepoHistories> {
        self.graph.analyze_histories(&self.repo)
    }

    pub fn default_dev_rc_info(&self) -> RcCommitInfo {
        let mut rcinfo = RcCommitInfo::default();

        for proj in self.graph.projects() {
            rcinfo.projects.push(RcProjectInfo {
                qnames: proj.qualified_names().to_owned(),
                bump_spec: "dev-datecode".to_owned(),
            })
        }

        rcinfo
    }

    /// Rewrite all packages' changelogs to include their full release-branch
    /// content. Packages staged for release will have new entries created
    /// giving their final version numbers and other release information.
    pub fn apply_changelogs(
        &self,
        latest_release_commit: Option<CommitId>,
        rcinfo: &RcCommitInfo,
        changes: &mut ChangeList,
    ) -> Result<()> {
        // This step could plausibly be implemented in the "rewriter" framework,
        // probably? I dodn't have a great reason for doing otherwise, other
        // than that it seemed easier at the time.

        for ident in self.graph.toposorted() {
            let proj = self.graph.lookup(ident);

            if rcinfo.lookup_project(proj).is_some() {
                proj.changelog
                    .finalize_changelog(proj, &self.repo, changes)?;
            } else if let Some(cid) = latest_release_commit {
                // If the project is not being released, we still have to copy
                // out its most recent changelog so as not to lose it from the
                // release branch.
                proj.changelog.replace_changelog(proj, self, changes, cid)?;
            }
        }

        Ok(())
    }

    /// Create version control tags for new releases.
    pub fn create_tags(&mut self, rel_info: &ReleaseCommitInfo) -> Result<()> {
        for proj in self.graph.toposorted_mut() {
            if let Some(rel) = rel_info.lookup_if_released(proj) {
                self.repo.tag_project_at_head(proj, rel)?;
            }
        }

        Ok(())
    }
}

/// Different categorizations of the environment in which the program is
/// running.
pub enum ExecutionEnvironment {
    /// The program is running in a CI environment, in "release request" mode
    /// where we have not yet created a "release commit" with final version
    /// number information. If the boolean is true, we are in a development mode
    /// where version numbers are temporary and release artifacts will not be
    /// deployed.
    CiRcMode(bool, RcCommitInfo),

    /// The program is running in a CI environment, in a "release deployment"
    /// mode where HEAD is a Cranko release commit. If the boolean is true, we
    /// are in a development mode where version numbers are temporary and
    /// release artifacts will not be deployed (but this mode still can be
    /// useful for creating artifacts and so on).
    CiReleaseMode(bool, ReleaseCommitInfo),

    /// The program does not appear to be running in a CI environment. We infer
    /// that we're running in an individual development environment.
    NotCi,
}
