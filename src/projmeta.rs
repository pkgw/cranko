// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Project metadata files.

use crate::{app::{AppSession, RepoPath}, errors::Result};

pub mod cargo;

/// A trait for different kinds of files that contain project metadata.
///
/// Examples might be `setup.py`, `Cargo.toml`, or `package.json`.
pub trait ProjectMetadata: std::fmt::Debug {
    /// Assuming that a project is anchored at the given path prefix, load
    /// metadata files and create an instance of this metadata type. Returns
    /// None if closer examination shows that this directory does not correspond
    /// to a project (e.g., there's a Cargo.toml but it's a workspace-only
    /// file). The repo_path is empty if we’re looking at the root directory;
    /// otherwise it will end in a path separator.
    fn new_from_prefix(sess: &AppSession, repo_path: &RepoPath) -> Result<Option<Self>>
    where
        Self: Sized;

    /// Get a textual name for this project.
    fn project_name(&self) -> &str;
}
