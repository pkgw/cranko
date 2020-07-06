// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Project metadata stored in a `Cargo.toml` file.

use std::{fs::File, io::Read};
use toml_edit::Document;

use super::ProjectMetadata;
use crate::{app::{AppSession, RepoPath}, errors::Result};

/// The type for Cargo.toml project metadata.
#[derive(Debug)]
pub struct CargoMetadata {}

impl ProjectMetadata for CargoMetadata {
    fn new_from_prefix(sess: &AppSession, repo_path: &RepoPath) -> Result<Option<Self>> {
        let text = {
            let mut p = sess.resolve_workdir(repo_path);
            p.push("Cargo.toml");
            let mut f = File::open(&p)?;
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            s
        };
        let doc = text.parse::<Document>()?;

        Ok(Some(CargoMetadata {}))
    }

    fn project_name(&self) -> &str {
        "my-awesome-cargo-project"
    }
}
