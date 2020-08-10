// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Information about a single project within the repository.
//!
//! Here, a project is defined as something that’s assigned version numbers.
//! Many repositories contain only a single project, but in the general case
//! (i.e., a monorepo) there can be many projects within a single repo, with
//! interdependencies inducing a Directed Acyclic Graph (DAG) structure on them,
//! as implemented in the `graph` module.

use crate::{
    changelog::ChangelogFormat,
    graph::ProjectGraph,
    repository::{RepoPath, RepoPathBuf},
    rewriters::Rewriter,
    version::{ReleaseMode, Version, VersioningScheme},
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

    /// The number of commits at which this project has kept the same version.
    /// This isn't meant to be used in most cases, but it helps us with the
    /// "project info" bookkeeping that we do in the commit messages on the
    /// release branch.
    pub version_age: usize,

    /// The versioning scheme to use in ReleaseMode::Development releases.
    dev_scheme: VersioningScheme,

    /// The versioning scheme to use in ReleaseMode::Primary releases.
    primary_scheme: VersioningScheme,

    /// Steps to perform when rewriting this project's metadata to produce
    /// a release commit.
    pub rewriters: Vec<Box<dyn Rewriter>>,

    /// A directory prefix in the repository: any commits that affect files
    /// within this prefix are assumed to affect this project. Should be empty
    /// if all commits affect this project. Otherwise, should end with a
    /// trailing slash for easy path combination.
    prefix: RepoPathBuf,

    /// How this project's changelog is formatted and updated.
    pub changelog: ChangelogFormat,
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

    /// Get the versioning scheme used by this project for specific release mode.
    pub fn versioning_scheme(&self, mode: ReleaseMode) -> VersioningScheme {
        match mode {
            ReleaseMode::Development => self.dev_scheme,
            ReleaseMode::Primary => self.primary_scheme,
        }
    }

    /// Get this project's prefix in the repository filesystem.
    pub fn prefix(&self) -> &RepoPath {
        &self.prefix
    }
}

/// A builder for initializing a new project entry that will be added to the
/// graph.
///
/// Note that you can also mutate Projects after they have been created, so not
/// all possible settings are exposed in this interface. This builder exists to
/// initialize the fields in Project that (1) are required and (2) do not have
/// "sensible" defaults.
#[derive(Debug)]
pub struct ProjectBuilder<'a> {
    owner: &'a mut ProjectGraph,
    qnames: Vec<String>,
    version: Option<Version>,
    prefix: Option<RepoPathBuf>,
}

impl<'a> ProjectBuilder<'a> {
    #[doc(hidden)]
    pub fn new(owner: &'a mut ProjectGraph) -> Self {
        ProjectBuilder {
            owner,
            qnames: Vec::new(),
            version: None,
            prefix: None,
        }
    }

    /// Set the qualified names associated with the project to be created.
    pub fn qnames<T: std::fmt::Display>(
        &mut self,
        qnames: impl IntoIterator<Item = T>,
    ) -> &mut Self {
        self.qnames = qnames.into_iter().map(|s| s.to_string()).collect();
        self
    }

    /// Set the current version number associated with the project to be created.
    pub fn version(&mut self, version: Version) -> &mut Self {
        self.version = Some(version);
        self
    }

    /// Set the repository file prefix associated with the project to be created.
    pub fn prefix(&mut self, prefix: RepoPathBuf) -> &mut Self {
        self.prefix = Some(prefix);
        self
    }

    /// Add the template project to the graph, consuming this object and
    /// returning its unique ID that can be used to apply further settings.
    pub fn finish_init(self) -> ProjectId {
        assert!(self.qnames.len() > 0);
        let qnames = self.qnames;

        let version = self.version.unwrap();
        let prefix = self.prefix.unwrap();

        self.owner.finalize_project_addition(|ident| Project {
            ident,
            qnames: qnames,
            user_facing_name: String::new(),
            version,
            version_age: 0,
            dev_scheme: VersioningScheme::DevDatecode,
            primary_scheme: VersioningScheme::DevDatecode, // XXX update
            rewriters: Vec::new(),
            prefix,
            changelog: ChangelogFormat::default(),
        })
    }
}
