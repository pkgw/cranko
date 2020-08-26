// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Project metadata stored in a `Cargo.toml` file.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
};
use toml_edit::Document;

use super::Rewriter;

use crate::{
    app::AppSession,
    errors::{Error, Result},
    project::ProjectId,
    repository::{ChangeList, RepoPathBuf},
    version::Version,
};

#[derive(Debug)]
pub struct CargoRewriter {
    proj_id: ProjectId,
    toml_path: RepoPathBuf,
}

impl CargoRewriter {
    /// Create a new Cargo.toml rewriter.
    pub fn new(proj_id: ProjectId, toml_path: RepoPathBuf) -> Self {
        CargoRewriter { proj_id, toml_path }
    }
}

impl Rewriter for CargoRewriter {
    fn rewrite(&self, app: &AppSession, changes: &mut ChangeList) -> Result<()> {
        // Parse the current Cargo.toml using toml_edit so we can rewrite it
        // with minimal deltas.
        let toml_path = app.repo.resolve_workdir(&self.toml_path);
        let mut s = String::new();
        {
            let mut f = File::open(&toml_path)?;
            f.read_to_string(&mut s)?;
        }
        let mut doc: Document = s.parse()?;

        // Helper table for applying internal deps. Note that we use the 0'th
        // qname, not the user-facing name, since that is what is used in
        // Cargo-land.

        let proj = app.graph().lookup(self.proj_id);
        let mut internal_reqs = HashMap::new();

        for req in &proj.internal_reqs[..] {
            internal_reqs.insert(
                app.graph().lookup(req.ident).qualified_names()[0].clone(),
                req.min_version.clone(),
            );
        }

        // Update the project version

        {
            let ct_root = doc.as_table_mut();
            let ct_package = ct_root.entry("package").as_table_mut().ok_or_else(|| {
                Error::RewriteFormatError(format!(
                    "no [package] section in {}?!",
                    self.toml_path.escaped()
                ))
            })?;

            ct_package["version"] = toml_edit::value(proj.version.to_string());

            // Rewrite any internal dependencies. These may be found in three
            // main tables and a nested table of potential target-specific
            // tables.

            for tblname in &["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(tbl) = ct_root.entry(tblname).as_table_mut() {
                    rewrite_deptable(&internal_reqs, tbl)?;
                }
            }

            if let Some(ct_target) = ct_root.entry("target").as_table_mut() {
                // As far as I can tell, no way to iterate over the table while mutating
                // its values?
                let target_specs = ct_target
                    .iter()
                    .map(|(k, _v)| k.to_owned())
                    .collect::<Vec<_>>();

                for target_spec in &target_specs[..] {
                    if let Some(tbl) = ct_target.entry(target_spec).as_table_mut() {
                        rewrite_deptable(&internal_reqs, tbl)?;
                    }
                }
            }
        }

        fn rewrite_deptable(
            internal_reqs: &HashMap<String, Version>,
            tbl: &mut toml_edit::Table,
        ) -> Result<()> {
            let deps = tbl.iter().map(|(k, _v)| k.to_owned()).collect::<Vec<_>>();

            for dep in &deps[..] {
                // ??? renamed internal deps? We could save rename informaion
                // from cargo-metadata when we load everything.

                if let Some(min_version) = internal_reqs.get(dep) {
                    if let Some(dep_tbl) = tbl.entry(dep).as_table_mut() {
                        dep_tbl["version"] = toml_edit::value(format!("^{}", min_version));
                    } else if let Some(dep_tbl) = tbl.entry(dep).as_inline_table_mut() {
                        // Can't just index inline tables???
                        if let Some(val) = dep_tbl.get_mut("version") {
                            *val = format!("^{}", min_version).into();
                        } else {
                            dep_tbl.get_or_insert("version", format!("^{}", min_version));
                        }
                    } else {
                        return Err(Error::Environment(format!(
                            "unexpected internal dependency item in a Cargo.toml: {:?}",
                            tbl.entry(dep)
                        )));
                    }
                }
            }

            Ok(())
        }

        // Rewrite.

        {
            let mut f = File::create(&toml_path)?;
            write!(f, "{}", doc.to_string_in_original_order())?;
            changes.add_path(&self.toml_path);
        }

        Ok(())
    }
}
