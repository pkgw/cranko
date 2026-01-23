// Copyright Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! "Generic" projects that do not interact with any external tool. These
//! basically are just a vessel that allows us to attach a version number to
//! some set of files.
//!
//! This could get more sophisticated, but for now it's quite limited.

use log::warn;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Read},
};

use crate::{
    app::{AppBuilder, AppSession},
    atry,
    config::ProjectConfiguration,
    errors::Result,
    project::ProjectId,
    repository::{ChangeList, RepoPath, RepoPathBuf},
    rewriters::Rewriter,
    version::Version,
    write_crlf,
};

/// Framework for auto-loading Visual Studio C# projects from the repository
/// contents.
#[derive(Debug, Default)]
pub struct GenericLoader {
    dirs_of_interest: HashSet<RepoPathBuf>,
}

impl GenericLoader {
    pub fn process_index_item(&mut self, dirname: &RepoPath, basename: &RepoPath) -> Result<()> {
        if basename.as_ref() == b"CrankoProject.toml" {
            self.dirs_of_interest.insert(dirname.to_owned());
        }

        Ok(())
    }

    /// Finalize autoloading any generic projects. Consumes this object.
    pub fn finalize(
        mut self,
        app: &mut AppBuilder,
        pconfig: &HashMap<String, ProjectConfiguration>,
    ) -> Result<()> {
        for dirname in self.dirs_of_interest.drain() {
            let mut toml_repopath = dirname.clone();
            toml_repopath.push("CrankoProject.toml");

            let mut config: GenericProjectFile = {
                let toml_path = app.repo.resolve_workdir(&toml_repopath);
                let mut f = atry!(
                    File::open(&toml_path);
                    ["failed to open file `{}`", toml_path.display()]
                );

                let mut text = String::new();
                atry!(
                    f.read_to_string(&mut text);
                    ["failed to read file `{}`", toml_path.display()]
                );

                atry!(
                    toml::from_str(&text);
                    ["could not parse file `{}` as TOML", toml_path.display()]
                )
            };

            // Registering is easy

            let qnames = vec![config.name.to_owned(), "generic".to_owned()];

            if let Some(ident) = app.graph.try_add_project(qnames, pconfig) {
                let proj = app.graph.lookup_mut(ident);
                proj.prefix = Some(dirname.to_owned());
                proj.version = Some(Version::Semver(semver::Version::new(0, 0, 0)));

                for spec in config.rewrite.drain(..) {
                    let rewrite = GenericRewriter::new(ident, dirname.to_owned(), spec);
                    proj.rewriters.push(Box::new(rewrite));
                }
            }
        }

        // For now (?), no interdependencies.
        Ok(())
    }
}

/// Toplevel `CrankoProject.toml` deserialization container.
#[derive(Debug, Deserialize)]
struct GenericProjectFile {
    pub name: String,
    pub rewrite: Vec<GenericRewriteSpec>,
}

/// A block describing how a set of files should be rewritten
#[derive(Debug, Deserialize)]
struct GenericRewriteSpec {
    pub version_placeholder: String,
    pub files: Vec<String>,
}

/// The generic rewriter
#[derive(Debug)]
pub struct GenericRewriter {
    proj_id: ProjectId,
    proj_root: RepoPathBuf,
    spec: GenericRewriteSpec,
}

impl GenericRewriter {
    fn new(proj_id: ProjectId, proj_root: RepoPathBuf, spec: GenericRewriteSpec) -> Self {
        GenericRewriter {
            proj_id,
            proj_root,
            spec,
        }
    }
}

impl Rewriter for GenericRewriter {
    fn rewrite(&self, app: &AppSession, changes: &mut ChangeList) -> Result<()> {
        let proj = app.graph().lookup(self.proj_id);
        let version = proj.version.to_string();

        for rel_path in &self.spec.files {
            let mut did_anything = false;
            let mut repo_path = self.proj_root.clone();
            repo_path.push(rel_path);
            let file_path = app.repo.resolve_workdir(&repo_path);

            let cur_f = atry!(
                File::open(&file_path);
                ["failed to open file `{}` for reading", file_path.display()]
            );
            let cur_reader = BufReader::new(cur_f);

            let new_af = atomicwrites::AtomicFile::new(
                &file_path,
                atomicwrites::OverwriteBehavior::AllowOverwrite,
            );

            let r = new_af.write(|new_f| {
                for line in cur_reader.lines() {
                    let mut line = atry!(
                        line;
                        ["error reading data from file `{}`", file_path.display()]
                    );

                    if line.contains(&self.spec.version_placeholder) {
                        did_anything = true;
                        line = line.replace(&self.spec.version_placeholder, &version);
                    }

                    atry!(
                        write_crlf!(new_f, "{}", line);
                        ["error writing data to `{}`", new_af.path().display()]
                    );
                }

                Ok(())
            });

            match r {
                Err(atomicwrites::Error::Internal(e)) => return Err(e.into()),
                Err(atomicwrites::Error::User(e)) => return Err(e),
                Ok(()) => {}
            };

            if !did_anything {
                warn!(
                    "generic rewriter for file `{}` didn't make any modifications",
                    file_path.display()
                );
            }

            changes.add_path(&repo_path);
        }

        Ok(())
    }
}
