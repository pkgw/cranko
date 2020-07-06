// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Information about a single project within the repository.
//!
//! Here, a project is defined as something that’s assigned version numbers.
//! Many repositories contain only a single project, but in the general case
//! (i.e., a monorepo) there can be many projects within a single repo, with
//! interdependencies inducing a Directed Acyclic Graph (DAG) structure on them,
//! as implemented in the `graph` module.

use crate::{app::RepoPath, errors::Result, projmeta::ProjectMetadata};

pub type ProjectId = usize;

#[derive(Debug)]
pub struct Project {
    ident: ProjectId,
    meta: Box<dyn ProjectMetadata>,
}

impl Project {
    /// The `repo_path` prefix will either be empty (for the top level of the
    /// repo) or end with a directory separator.
    pub fn new_from_prefix<T: 'static + ProjectMetadata>(
        ident: ProjectId,
        repo_path: &RepoPath,
    ) -> Result<Option<Self>> {
        let meta = match T::new_from_prefix(repo_path)? {
            None => return Ok(None),
            Some(m) => m,
        };

        Ok(Some(Project {
            ident,
            meta: Box::new(meta),
        }))
    }

    /// Get the internal unique identifier of this project.
    ///
    /// These identifiers should not be persisted and are not guaranteed to have
    /// any particular semantics other than being cheaply copyable.
    pub fn ident(&self) -> ProjectId {
        self.ident
    }

    /// Get a textual name for this project.
    pub fn name(&self) -> &str {
        self.meta.project_name()
    }
}
