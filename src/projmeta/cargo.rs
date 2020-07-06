// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Project metadata stored in a `Cargo.toml` file.

use super::ProjectMetadata;
use crate::{app::RepoPath, errors::Result};

/// The type for Cargo.toml project metadata.
#[derive(Debug)]
pub struct CargoMetadata {}

impl ProjectMetadata for CargoMetadata {
    fn new_from_prefix(repo_path: &RepoPath) -> Result<Option<Self>> {
        Ok(Some(CargoMetadata {}))
    }

    fn project_name(&self) -> &str {
        "my-awesome-cargo-project"
    }
}
