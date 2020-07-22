// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Information about a single project within the repository.
//!
//! Here, a project is defined as something that’s assigned version numbers.
//! Many repositories contain only a single project, but in the general case
//! (i.e., a monorepo) there can be many projects within a single repo, with
//! interdependencies inducing a Directed Acyclic Graph (DAG) structure on them,
//! as implemented in the `graph` module.

use crate::{app::AppSession, errors::Result, graph::ProjectGraph, repository::RepoPath};

/// An internal, unique identifier for a project in this app session.
///
/// These identifiers should not be persisted and are not guaranteed to have any
/// particular semantics other than being cheaply copyable.
pub type ProjectId = usize;

/// A version number associated with a project.
///
/// This is an enumeration because different kinds of projects may subscribe to
/// different kinds of versioning schemes.
#[derive(Debug, Eq, PartialEq)]
pub enum Version {
    Semver(semver::Version),
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Version::Semver(ref v) => write!(f, "{}", v),
        }
    }
}

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

    /// The version associated with this project.
    pub version: Version,
}

impl Project {
    /// Get the internal unique identifier of this project.
    ///
    /// These identifiers should not be persisted and are not guaranteed to have
    /// any particular semantics other than being cheaply copyable.
    pub fn ident(&self) -> ProjectId {
        self.ident
    }

    /// Get the name of the project as we'll show it to the user.
    ///
    /// This is not necessarily straightforward since a repository might contain
    /// multiple projects with names that need distinguishing; e.g. a repository
    /// with related Python and NPM packages that have the same name on their
    /// respective registries.
    pub fn user_facing_name(&self) -> &str {
        &self.qnames[0] // XXXX DO BETTER
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
}

impl<'a> ProjectBuilder<'a> {
    #[doc(hidden)]
    pub fn new(owner: &'a mut ProjectGraph) -> Self {
        ProjectBuilder {
            owner,
            qnames: Vec::new(),
            version: None,
        }
    }

    pub fn qnames<T: std::fmt::Display>(
        &mut self,
        qnames: impl IntoIterator<Item = T>,
    ) -> &mut Self {
        self.qnames = qnames.into_iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn version(&mut self, version: Version) -> &mut Self {
        self.version = Some(version);
        self
    }

    pub fn finish_init(self) -> ProjectId {
        assert!(self.qnames.len() > 0);
        let qnames = self.qnames;

        let version = self.version.unwrap();

        self.owner.finalize_project_addition(|ident| Project {
            ident,
            qnames: qnames,
            version,
        })
    }
}
