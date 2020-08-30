// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State for the Cranko CLI application.

use log::{error, info, warn};
use std::collections::HashMap;

use crate::{
    errors::{Error, Result},
    graph::{ProjectGraph, RepoHistories},
    project::{ProjectId, ResolvedRequirement},
    repository::{
        ChangeList, CommitAvailability, PathMatcher, RcCommitInfo, RcProjectInfo,
        ReleaseCommitInfo, Repository,
    },
    version::Version,
};

/// The main Cranko CLI application state structure.
pub struct AppSession {
    /// The backing repository.
    pub repo: Repository,

    /// The graph of projects contained within the repo.
    graph: ProjectGraph,

    /// Information about the CI environment that we may be running in.
    ci_info: ci_info::types::CiInfo,
}

impl AppSession {
    /// Initialize a new application session.
    ///
    /// Initialization may fail if the environment doesn't associate the process
    /// with a proper Git repository with a work tree.
    pub fn initialize() -> Result<AppSession> {
        let repo = Repository::open_from_env()?;
        let graph = ProjectGraph::default();
        let ci_info = ci_info::get();

        Ok(AppSession {
            graph,
            repo,
            ci_info,
        })
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

                return Ok(ExecutionEnvironment::CiPullRequest);
            }

            if maybe_ci_branch == Some(release_name) {
                warn!("cranko seems to be running in an update to the `{}` branch; this is not recommended", release_name);
                warn!("... treating as a non-CI environment for safety");
                return Ok(ExecutionEnvironment::NotCi);
            }

            if maybe_ci_branch != Some(rc_name) {
                return Ok(ExecutionEnvironment::CiDevelopmentBranch);
            }

            // OK, we're in CI triggered by a push to the `rc` branch. We allow
            // two further possibilities: that we are still on the `rc` branch,
            // so that the HEAD commit should contain release request
            // information; or that the releases have been approved and we have
            // subsequently switched to the `release` branch, so that the HEAD
            // commit should contain approved-release information.

            if let Some(current_branch) = self.repo.current_branch_name()? {
                if current_branch == rc_name {
                    Ok(ExecutionEnvironment::CiRcMode(
                        self.repo.parse_rc_info_from_head()?,
                    ))
                } else if current_branch == release_name {
                    Ok(ExecutionEnvironment::CiReleaseMode(
                        self.repo.parse_release_info_from_head()?,
                    ))
                } else {
                    Err(Error::Environment(format!(
                        "unexpected checked-out branch name `{}` in a CI update to the `{}` branch",
                        current_branch, rc_name
                    )))
                }
            } else {
                Err(Error::Environment(format!(
                    "cannot determine checked-out branch in a CI update to the `{}` branch",
                    rc_name
                )))
            }
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
                    Err(Error::Environment(
                        "refusing to proceed (use \"force\" mode to override)".to_owned(),
                    ))
                }
            }
        }
    }

    /// Check that the current process is running in the "release mode" CI
    /// environment, returning the latest release information. Any other
    /// circumstance results in an error.
    pub fn ensure_ci_release_mode(&self) -> Result<ReleaseCommitInfo> {
        match self.execution_environment()? {
            ExecutionEnvironment::NotCi => {
                error!("no CI environment detected; this is unexpected for this command");
                Err(Error::Environment(
                    "don't know how to obtain release information -- cannot proceed".to_owned(),
                ))
            }

            ExecutionEnvironment::CiReleaseMode(ri) => Ok(ri),

            _ => {
                error!("unexpected CI environment detected");
                error!("... this command should only be run on updates to the `rc`-type branch");
                error!("... after switching to a local `release`-type branch");
                Err(Error::Environment(
                    "don't know how to obtain release information -- cannot proceed".to_owned(),
                ))
            }
        }
    }

    /// Check that the current process is running and "RC"-like CI mode: either
    /// an update to the `rc` branch, before release deployment processes have
    /// activated; or in a pull request or push to some other branch, assumed to
    /// be a standard development branch.
    ///
    /// The returned boolean is true if in a "development"-like mode, false if
    /// in the intended `rc` mode.
    pub fn ensure_ci_rc_like_mode(&self, force: bool) -> Result<(bool, RcCommitInfo)> {
        match self.execution_environment()? {
            ExecutionEnvironment::CiRcMode(rci) => Ok((false, rci)),

            ExecutionEnvironment::CiPullRequest | ExecutionEnvironment::CiDevelopmentBranch => {
                Ok((true, self.default_dev_rc_info()))
            }

            ExecutionEnvironment::CiReleaseMode(_) => {
                warn!("unexpected CI environment detected");
                warn!("... this command should only be run on updates to the `rc`-type branch");
                if force {
                    Ok((true, self.default_dev_rc_info()))
                } else {
                    Err(Error::Environment(
                        "refusing to proceed (use \"force\" mode to override)".to_owned(),
                    ))
                }
            }

            ExecutionEnvironment::NotCi => {
                warn!("no CI environment detected; this is unexpected for this command");
                if force {
                    Ok((true, self.default_dev_rc_info()))
                } else {
                    Err(Error::Environment(
                        "refusing to proceed (use \"force\" mode to override)".to_owned(),
                    ))
                }
            }
        }
    }

    /// Check that the working tree is completely clean. We allow untracked and
    /// ignored files but otherwise don't want any modifications, etc. Returns Ok
    /// if clean, Err if not.
    pub fn ensure_fully_clean(&self) -> Result<()> {
        if let Some(changed_path) = self.repo.check_if_dirty(&[])? {
            Err(Error::DirtyRepository(changed_path))
        } else {
            Ok(())
        }
    }

    /// Check that the working tree is clean, excepting modifications to any
    /// files interpreted as changelogs. Returns Ok if clean, Err if not.
    pub fn ensure_changelog_clean(&self) -> Result<()> {
        let mut matchers: Vec<Result<PathMatcher>> = self
            .graph
            .projects()
            .map(|p| p.changelog.create_path_matcher(p))
            .collect();
        let matchers: Result<Vec<PathMatcher>> = matchers.drain(..).collect();
        let matchers = matchers?;

        if let Some(changed_path) = self.repo.check_if_dirty(&matchers[..])? {
            Err(Error::DirtyRepository(changed_path))
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

    /// Get the graph of projects inside this app session.
    ///
    /// If the graph has not yet been loaded, this triggers processing of the
    /// config file and repository to fill in the graph information, hence the
    /// fallibility.
    pub fn populated_graph(&mut self) -> Result<&ProjectGraph> {
        if self.graph.len() == 0 {
            self.populate_graph()?;
        }

        Ok(&self.graph)
    }

    fn populate_graph(&mut self) -> Result<()> {
        // Start by auto-detecting everything in the repo index.

        let mut cargo = crate::loaders::cargo::CargoLoader::default();

        self.repo.scan_paths(|p| {
            let (dirname, basename) = p.split_basename();
            cargo.process_index_item(dirname, basename);
        })?;

        cargo.finalize(self)?;

        self.graph.complete_loading()?;
        Ok(())
    }

    /// Apply version numbers given the current repository state and bump
    /// specifications.
    ///
    /// This also involves solving the version requirements for internal
    /// dependencies.
    pub fn apply_versions(&mut self, rc_info: &RcCommitInfo) -> Result<()> {
        let mut new_versions: HashMap<ProjectId, Version> = HashMap::new();
        let latest_info = self.repo.get_latest_release_info()?;

        for ident in self.graph.toposort_idents()? {
            // First, make sure that we can satisfy this project's internal
            // dependencies. By definition of the toposort, any of its
            // dependencies will have already been visited.
            let deps = self.graph.resolve_direct_dependencies(&self.repo, ident)?;
            let proj = self.graph.lookup_mut(ident);

            for dep in &deps[..] {
                let min_version = match dep.availability {
                    CommitAvailability::NotAvailable => {
                        return Err(Error::UnsatisfiedInternalRequirement(
                            proj.user_facing_name.to_string(),
                        ))
                    }

                    CommitAvailability::ExistingRelease(ref v) => v.clone(),

                    CommitAvailability::NewRelease => {
                        if let Some(v) = new_versions.get(&dep.ident) {
                            v.clone()
                        } else {
                            return Err(Error::UnsatisfiedInternalRequirement(
                                proj.user_facing_name.to_string(),
                            ));
                        }
                    }
                };

                proj.internal_reqs.push(ResolvedRequirement {
                    ident: dep.ident,
                    min_version,
                });
            }

            let cur_version = proj.version.clone();
            let latest_release = latest_info.lookup_project(proj);

            if let Some(rc) = rc_info.lookup_project(proj) {
                let scheme = proj.version.parse_bump_scheme(&rc.bump_spec)?;
                proj.version = scheme.apply(&cur_version, latest_release)?;
                new_versions.insert(proj.ident(), proj.version.clone());
                info!(
                    "{}: {} => {}",
                    proj.user_facing_name, cur_version, proj.version
                );
            }

            // Bookkeeping so that we can produce updated release info.
            proj.version_age = match (latest_release, proj.version == cur_version) {
                (Some(info), true) => info.age + 1,
                _ => 0,
            };
        }

        Ok(())
    }

    /// Rewrite everyone's metadata to match our internal state.
    pub fn rewrite(&self) -> Result<ChangeList> {
        let mut changes = ChangeList::default();

        for ident in self.graph.toposort_idents()? {
            let proj = self.graph.lookup(ident);

            for rw in &proj.rewriters {
                rw.rewrite(self, &mut changes)?;
            }
        }

        Ok(changes)
    }

    pub fn make_release_commit(&mut self) -> Result<()> {
        self.repo.make_release_commit(&self.graph)
    }

    pub fn make_rc_commit(
        &mut self,
        rcinfo: Vec<RcProjectInfo>,
        changes: &ChangeList,
    ) -> Result<()> {
        self.repo.make_rc_commit(rcinfo, &changes)?;
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

    // Rewrite the changelogs of packages staged for release to include their
    // final version numbers and other release information.
    pub fn apply_changelogs(&self, rcinfo: &RcCommitInfo, changes: &mut ChangeList) -> Result<()> {
        // This step could plausibly be implemented in the "rewriter" framework,
        // probably? I dodn't have a great reason for doing otherwise, other
        // than that it seemed easier at the time.

        for proj in self.graph.toposort()? {
            if let Some(_) = rcinfo.lookup_project(proj) {
                proj.changelog
                    .finalize_changelog(proj, &self.repo, changes)?;
            }
        }

        Ok(())
    }

    /// Create version control tags for new releases.
    pub fn create_tags(&mut self, rel_info: &ReleaseCommitInfo) -> Result<()> {
        self.populate_graph()?;

        for proj in self.graph.toposort_mut()? {
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
    /// This program is running in a CI environment, in response to an
    /// externally submitted pull request.
    CiPullRequest,

    /// The program is running in a CI environment, in response to an update to
    /// the main development branch (e.g., `master`).
    CiDevelopmentBranch,

    /// The program is running in a CI environment, in response to an update to
    /// the `rc`-type branch, and the current branch is still `rc`. Therefore,
    /// the HEAD commit should be associated with release request information.
    CiRcMode(RcCommitInfo),

    /// The program is running in a CI environment, in response to an update to
    /// the `rc`-type branch, and the current branch has been changed to
    /// `release`. This means that the CI tests passed, and the release
    /// deployment process is now underway. Therefore, the HEAD commit should be
    /// associated with full release information.
    CiReleaseMode(ReleaseCommitInfo),

    /// The program does not appear to be running in a CI environment. We infer
    /// that we're running in an individual development environment.
    NotCi,
}
