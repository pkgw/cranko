// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Information about a single project within the repository.
//!
//! Here, a project is defined as something that’s assigned version numbers.
//! Many repositories contain only a single project, but in the general case
//! (i.e., a monorepo) there can be many projects within a single repo, with
//! interdependencies inducing a Directed Acyclic Graph (DAG) structure on them,
//! as implemented in the `graph` module.

use anyhow::{anyhow, bail};

use crate::{
    changelog::{self, Changelog},
    errors::Result,
    repository::{CommitId, PathMatcher, RepoPath, RepoPathBuf},
    rewriters::Rewriter,
    version::Version,
};

/// An internal, unique identifier for a project in this app session.
///
/// These identifiers should not be persisted and are not guaranteed to have any
/// particular semantics other than being cheaply copyable.
pub type ProjectId = usize;

#[derive(Debug)]
pub struct Project {
    ident: ProjectId,

    /// Qualified names. The package name, qualified with hierarchical
    /// indicators. The first item in the vector is the most specific name and
    /// the one that the user is most likely to recognize as corresponding to
    /// the project. Additional terms become more and more general and can be
    /// used to disambiguate packages originating from different schemes: e.g.,
    /// a repo containing related Python and NPM packages that both have the
    /// same name.
    qnames: Vec<String>,

    /// The user-facing package name; this is computed from the qualified names
    /// after all of the projects are loaded, so that we can make sure that
    /// these are unique.
    pub user_facing_name: String,

    /// The version associated with this project.
    pub version: Version,

    /// Steps to perform when rewriting this project's metadata to produce
    /// a release commit.
    pub rewriters: Vec<Box<dyn Rewriter>>,

    /// The project's unique prefix in the repository.
    ///
    /// Should be empty if the prefix is the project root. Otherwise, should end
    /// with a trailing slash for easy path combination.
    ///
    /// Note that actual path relevance matching should be done using the
    /// `repo_paths` field, to handle common cases where a project has
    /// sub-projects contained in subdirectories. When matching paths we will
    /// generally want to exclude the sub-projects, which requires more
    /// sophistication than a simple prefix match.
    prefix: RepoPathBuf,

    /// A data structure describing the paths inside the repository that are
    /// considered to affect this project.
    pub repo_paths: PathMatcher,

    /// How this project's changelog is formatted and updated.
    pub changelog: Box<dyn Changelog>,

    /// This project's internal dependencies.
    pub internal_deps: Vec<Dependency>,
}

impl Project {
    /// Get the internal unique identifier of this project.
    ///
    /// These identifiers should not be persisted and are not guaranteed to have
    /// any particular semantics other than being cheaply copyable.
    pub fn ident(&self) -> ProjectId {
        self.ident
    }

    /// Get a reference to this project's full qualified names.
    pub fn qualified_names(&self) -> &Vec<String> {
        &self.qnames
    }

    /// Get this project's prefix in the repository filesystem.
    ///
    /// To check whether a particular path is relevant to this project, use the
    /// `repo_paths` field, which will properly account for any projects in
    /// subdirectorie relative to this project.
    pub fn prefix(&self) -> &RepoPath {
        &self.prefix
    }
}

/// Metadata about internal interdependencies between projects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dependency {
    /// The project that is depended upon
    pub ident: ProjectId,

    /// The current expression of the requirement in the project metadata files.
    /// In normal operations this should be an explicit requirement on version
    /// "0.0.0-dev.0", or the equivalent, so that the project can be built on
    /// the main branch.
    pub literal: String,

    /// The logical expression of the requirement in Cranko's framework. Cranko
    /// prefers to express version dependencies in terms of commit IDs. Since
    /// this concept is (properly) not integrated into package manager metadata
    /// files, the information expressing the requirement must be recorded in
    /// Cranko-specific metadata that are different than the literal expression.
    pub cranko_requirement: DepRequirement,

    /// If the requirement is expressed as a DepRequirement::Commit, *and* we
    /// have resolved that requirement to a specific version of the dependee
    /// project, that version is stored here. None values could be found if the
    /// requirement is not a commit or if the resolution process hasn't
    /// occurred, or if resolution failed.
    pub resolved_version: Option<Version>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepRequirement {
    /// The depending project requires a version of the dependee project later
    /// than the specified commit.
    Commit(CommitId),

    /// The depending project requires some version of the dependee project that
    /// has been manually specified by the user. This is discouraged, but
    /// necessary to support to enable bootstrapping. Note that the value of
    /// this manual specification is not redundant with `Dependency::literal`:
    /// in steady-state, the former will be something like `0.0.0-dev.0` so that
    /// everyday builds can work, while this might be `^0.1` if the project
    /// requires that version of its dependency and 0.1 was released before
    /// Cranko was introduced.
    Manual(String),

    /// Cranko metadata are missing, so we can't process this dependency in the
    /// Cranko framework.
    Unavailable,
}

impl std::fmt::Display for DepRequirement {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DepRequirement::Commit(cid) => write!(f, "{cid} (commit)"),
            DepRequirement::Manual(t) => write!(f, "{t} (manual)"),
            DepRequirement::Unavailable => write!(f, "(unavailable)"),
        }
    }
}

/// A builder for initializing a new project entry that will be added to the
/// graph.
#[derive(Debug)]
pub struct ProjectBuilder {
    pub qnames: Vec<String>,
    pub version: Option<Version>,
    pub prefix: Option<RepoPathBuf>,
    pub rewriters: Vec<Box<dyn Rewriter>>,
    pub internal_deps: Vec<DependencyBuilder>,
}

/// An in-process dependency. We haven't necessarily yet resolved references to
/// project ids.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyBuilder {
    pub target: DependencyTarget,
    pub literal: String,
    pub cranko_requirement: DepRequirement,
    pub resolved_version: Option<Version>,
}

/// The target of a DependencyBuilder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DependencyTarget {
    /// The target expressed as user-specified text that will be resolved to a
    /// user-facing name. Use this for dependencies manually specified by the
    /// user that might refer to any packaging system.
    Text(String),

    /// The target expressed as a known ProjectId. This is generally only
    /// possible for dependencies within the same packaging system.
    Ident(ProjectId),
}

impl ProjectBuilder {
    #[doc(hidden)]
    pub(crate) fn new() -> Self {
        ProjectBuilder {
            qnames: Vec::new(),
            version: None,
            prefix: None,
            rewriters: Vec::new(),
            internal_deps: Vec::new(),
        }
    }

    #[doc(hidden)]
    pub(crate) fn finalize(
        self,
        ident: ProjectId,
        user_facing_name: String,
        internal_deps: Vec<Dependency>,
    ) -> Result<Project> {
        if self.qnames.is_empty() {
            bail!(
                "could not load project `{}`: never figured out its naming",
                user_facing_name
            );
        }

        let version = self.version.ok_or_else(|| {
            anyhow!(
                "could not load project `{}`: never figured out its version",
                user_facing_name
            )
        })?;

        let prefix = self.prefix.ok_or_else(|| {
            anyhow!(
                "could not load project `{}`: never figured out its directory prefix",
                user_facing_name
            )
        })?;

        Ok(Project {
            ident,
            qnames: self.qnames,
            user_facing_name,
            version,
            prefix: prefix.clone(),
            rewriters: self.rewriters,
            repo_paths: PathMatcher::new_include(prefix),
            changelog: changelog::default(),
            internal_deps,
        })
    }
}
