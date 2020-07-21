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
    app::{AppSession, RepoPath},
    errors::Result,
};

/// An internal, unique identifier for a project in this app session.
///
/// These identifiers should not be persisted and are not guaranteed to have any
/// particular semantics other than being cheaply copyable.
pub type ProjectId = usize;

#[derive(Debug, Eq, PartialEq)]
pub enum Version {
    Semver(semver::Version),
}

#[derive(Debug)]
pub struct Project {
    ident: ProjectId,
    name_hier: Vec<String>,
    version: Version,
}

impl Project {
    pub fn new(name_hier: Vec<String>, version: Version) -> Self {
        Project {
            ident: 0, // XXX??
            name_hier,
            version,
        }
    }

    /// Get the internal unique identifier of this project.
    ///
    /// These identifiers should not be persisted and are not guaranteed to have
    /// any particular semantics other than being cheaply copyable.
    pub fn ident(&self) -> ProjectId {
        self.ident
    }
}
