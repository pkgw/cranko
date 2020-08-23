// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State for the Cranko CLI application.

use log::warn;

use crate::{
    errors::{Error, Result},
    graph::{ProjectGraph, RepoHistories},
    repository::{
        ChangeList, PathMatcher, RcCommitInfo, RcProjectInfo, ReleaseCommitInfo, Repository,
    },
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
    pub fn execution_environment(&self) -> ExecutionEnvironment {
        if !self.ci_info.ci {
            ExecutionEnvironment::NotCi
        } else {
            let maybe_pr = self.ci_info.pr;
            let maybe_branch = self.ci_info.branch_name.as_ref().map(|s| s.as_ref());
            let rc_name = self.repo.upstream_rc_name();
            let release_name = self.repo.upstream_release_name();

            if maybe_branch.is_none() {
                warn!("cannot determine the current branch name in this CI environment");
            }

            if let Some(true) = maybe_pr {
                if maybe_branch == Some(rc_name) {
                    warn!("cranko seems to be running in a pull request to the `rc` branch; this is not recommended");
                }

                if maybe_branch == Some(release_name) {
                    warn!("cranko seems to be running in a pull request to the `release` branch; this is not recommended");
                }

                return ExecutionEnvironment::CiPullRequest;
            }

            if maybe_branch == Some(rc_name) {
                return ExecutionEnvironment::CiRcBranch;
            }

            if maybe_branch == Some(release_name) {
                return ExecutionEnvironment::CiReleaseBranch;
            }

            ExecutionEnvironment::CiDevelopmentBranch
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

    /// Apply version numbers given the current repository state and bump specifications.
    pub fn apply_versions(&mut self, rcinfo: &RcCommitInfo) -> Result<()> {
        let latest_info = self.repo.get_latest_release_info()?;

        for proj in self.graph.toposort_mut()? {
            let cur_version = proj.version.clone();
            let latest_release = latest_info.lookup_project(proj);

            if let Some(rc) = rcinfo.lookup_project(proj) {
                let scheme = proj.version.parse_bump_scheme(&rc.bump_spec)?;
                proj.version = scheme.apply(&cur_version, latest_release)?;
                println!(
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

        for proj in self.graph.toposort()? {
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

/// Different categorizations of the environment in which the program is running.
pub enum ExecutionEnvironment {
    /// This program is running in a CI environment, in response to an
    /// externally submitted pull request.
    CiPullRequest,

    /// The program is running in a CI environment, in response to an update to
    /// the main development branch (e.g. ,`master`).
    CiDevelopmentBranch,

    /// The program is running in a CI environment, in response to an update
    /// to the `rc`-type branch. The HEAD commit should include Cranko release
    /// request information.
    CiRcBranch,

    /// The program is running in a CI environment, on the `release`-type
    /// branch. The HEAD commit should include Cranko release information. In
    /// the Cranko model, CI should not be invoked upon updates to the release
    /// branch, but during `rc` processing `cranko apply` will switch the active
    /// branch from `rc` to `release`.
    CiReleaseBranch,

    /// The program does not appear to be running in a CI environment. We infer
    /// that we're running in an individual development environment.
    NotCi,
}
