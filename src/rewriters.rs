// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Updating project versioning metadata in the repository.

use crate::{app::AppSession, errors::Result, repository::ChangeList};

pub mod cargo;

/// A trait for something that can perform some kind of metadata rewriting.
pub trait Rewriter: std::fmt::Debug {
    fn rewrite(&self, app: &AppSession, changes: &mut ChangeList) -> Result<()>;
}
