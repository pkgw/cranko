// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Project metadata stored in a `Cargo.toml` file.
//!
//! If we detect a Cargo.toml in the repo root, we use `cargo metadata` to slurp
//! information about all of the crates and their interdependencies.

use cargo_metadata::MetadataCommand;
use log::warn;
use std::collections::HashMap;

use crate::{
    app::AppSession,
    errors::Result,
    repository::{RepoPath, RepoPathBuf},
    rewriters::cargo::CargoRewriter,
    version::Version,
};

#[derive(Debug)]
pub struct CargoLoader {
    shortest_toml_dirname: Option<RepoPathBuf>,
}

impl Default for CargoLoader {
    fn default() -> Self {
        CargoLoader {
            shortest_toml_dirname: None,
        }
    }
}

impl CargoLoader {
    /// Process items in the Git index while auto-loading projects. Since we use
    /// `cargo metadata` to get project information, all we do here is find the
    /// toplevel `Cargo.toml` file and assume that it represents a single
    /// project root, as far as Cargo is concerned. If you have some weird repo
    /// structure that doesn't have a single toplevel Cargo.toml (either a
    /// workspace, or a single project), we'll have trouble with that.
    pub fn process_index_item(&mut self, dirname: &RepoPath, basename: &RepoPath) {
        if basename.as_ref() != b"Cargo.toml" {
            return;
        }

        if let Some(ref mut prev) = self.shortest_toml_dirname {
            // Find the longest common prefix of the two dirnames.
            let bytes0: &[u8] = prev.as_ref();
            let bytes1: &[u8] = dirname.as_ref();
            let len = bytes0
                .iter()
                .zip(bytes1)
                .take_while(|&(a, b)| a == b)
                .count();
            prev.truncate(len);
        } else {
            self.shortest_toml_dirname = Some(dirname.to_owned());
        }
    }

    /// Finalize autoloading any Cargo projects. Consumes this object.
    ///
    /// If this repository contains one or more `Cargo.toml` files, the
    /// `cargo_metadata` crate will be used to load project information.
    pub fn finalize(self, app: &mut AppSession) -> Result<()> {
        let shortest_toml_dirname = match self.shortest_toml_dirname {
            Some(d) => d,
            None => return Ok(()),
        };

        let mut toml_path = app.repo.resolve_workdir(&shortest_toml_dirname);
        toml_path.push("Cargo.toml");
        let mut cmd = MetadataCommand::new();
        cmd.manifest_path(&toml_path);
        cmd.features(cargo_metadata::CargoOpt::AllFeatures);
        let cargo_meta = cmd.exec()?;

        // Fill in the packages

        let mut cargo_to_graph = HashMap::new();

        for pkg in &cargo_meta.packages {
            if pkg.source.is_some() {
                continue; // This is an external package; not to be tracked.
            }

            // Auto-register a rewriter to update this package's Cargo.toml.
            let manifest_repopath = app.repo.convert_path(&pkg.manifest_path)?;
            let (prefix, _) = manifest_repopath.split_basename();

            let mut pb = app.graph_mut().add_project();

            // Q: should we include a registry name as a qualifier?
            pb.qnames(&[&pkg.name, "cargo"])
                .version(Version::Semver(pkg.version.clone()));
            pb.prefix(prefix.to_owned());

            let ident = pb.finish_init();
            cargo_to_graph.insert(pkg.id.clone(), ident);

            // Auto-register a rewriter to update this package's Cargo.toml.
            let cargo_rewrite = CargoRewriter::new(ident, manifest_repopath);
            app.graph_mut()
                .lookup_mut(ident)
                .rewriters
                .push(Box::new(cargo_rewrite));
        }

        // Now establish the interdependencies.

        let mut cargoid_to_index = HashMap::new();

        for (index, pkg) in cargo_meta.packages[..].iter().enumerate() {
            cargoid_to_index.insert(pkg.id.clone(), index);
        }

        for node in &cargo_meta.resolve.unwrap().nodes {
            let pkg = &cargo_meta.packages[cargoid_to_index[&node.id]];

            if let Some(depender_id) = cargo_to_graph.get(&node.id) {
                let maybe_versions = pkg.metadata.get("internal_dep_versions");
                let manifest_repopath = app.repo.convert_path(&pkg.manifest_path)?;

                for dep in &node.deps {
                    if let Some(dependee_id) = cargo_to_graph.get(&dep.pkg) {
                        let min_version = maybe_versions
                            .and_then(|table| table.get(&dep.name))
                            .and_then(|nameval| nameval.as_str())
                            .map(|text| app.repo.parse_commit_ref(text))
                            .transpose()?
                            .map(|cref| app.repo.resolve_commit_ref(&cref, &manifest_repopath))
                            .transpose()?;

                        if min_version.is_none() {
                            warn!(
                                "missing or invalid key `internal_dep_versions.{}` in `{}`",
                                &dep.name,
                                pkg.manifest_path.display()
                            );
                            warn!("... this is needed to specify the oldest version of `{}` compatible with `{}`", 
                                &dep.name, &pkg.name);
                        }

                        app.graph_mut()
                            .add_dependency(*depender_id, *dependee_id, min_version);
                    }
                }
            }
        }

        Ok(())
    }
}
