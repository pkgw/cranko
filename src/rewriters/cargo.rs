// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Project metadata stored in a `Cargo.toml` file.

use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};
use toml_edit::Document;

use super::Rewriter;

use crate::{
    app::AppSession,
    errors::{Error, Result},
    project::ProjectId,
};

#[derive(Debug)]
pub struct CargoRewriter {
    proj_id: ProjectId,
    toml_path: PathBuf,
}

impl CargoRewriter {
    /// Create a new Cargo.toml rewriter.
    pub fn new(proj_id: ProjectId, toml_path: PathBuf) -> Self {
        CargoRewriter { proj_id, toml_path }
    }
}

impl Rewriter for CargoRewriter {
    fn rewrite(&self, app: &AppSession) -> Result<()> {
        // Parse the current Cargo.toml using toml_edit so we can rewrite it
        // with minimal deltas.
        let mut s = String::new();
        {
            let mut f = File::open(&self.toml_path)?;
            f.read_to_string(&mut s)?;
        }
        let mut doc: Document = s.parse()?;

        // Update the project version

        let proj = app.graph().lookup(self.proj_id);

        {
            let ct_root = doc.as_table_mut();
            let ct_package = ct_root.entry("package").as_table_mut().ok_or_else(|| {
                Error::RewriteFormatError(format!(
                    "no [package] section in {}?!",
                    &self.toml_path.display()
                ))
            })?;

            ct_package["version"] = toml_edit::value(proj.version.to_string());

            // TODO: internal dependencies!!!
        }

        // Rewrite.

        {
            let mut f = File::create(&self.toml_path)?;
            write!(f, "{}", doc.to_string_in_original_order())?;
        }

        Ok(())
    }
}
