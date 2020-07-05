// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State for the Cranko CLI application.

use git2::Repository;

use crate::errors::Result;

/// The main Cranko CLI application state structure.
pub struct App {
    /// The Git repository.
    pub repo: Repository,
}

impl App {
    pub fn initialize() -> Result<App> {
        let repo = Repository::open_from_env()?;

        Ok(App {
          repo
        })
    }
}