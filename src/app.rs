// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State for the Cranko CLI application.

use git2::Repository;
use std::path::{Path, PathBuf};

use crate::{
    errors::{Error, Result},
    graph::ProjectGraph,
    project::Project,
};

/// The main Cranko CLI application state structure.
pub struct AppSession {
    /// The Git repository.
    repo: Repository,

    /// The graph of projects contained within the repo.
    graph: ProjectGraph,

    /// The projects. Projects are uniquely identified by their index into this
    /// vector.
    projects: Vec<Project>,
}

impl AppSession {
    /// Initialize a new application session.
    ///
    /// Initialization may fail if the process is not running inside a Git
    /// repository.
    pub fn initialize() -> Result<AppSession> {
        let repo = Repository::open_from_env()?;

        if repo.is_bare() {
            return Err(Error::BareRepository);
        }

        let graph = ProjectGraph::default();
        let projects = Vec::new();

        Ok(AppSession {
            graph,
            repo,
            projects,
        })
    }

    /// Resolve a repository path to a filesystem path in the working directory.
    pub fn resolve_workdir(&self, p: &RepoPath) -> PathBuf {
        let mut fullpath = self.repo.workdir().unwrap().to_owned();
        fullpath.push(p.as_path());
        fullpath
    }

    /// Get the graph of projects inside this app session.
    ///
    /// If the graph has not yet been loaded, this triggers processing of the
    /// config file and repository to fill in the graph information, hence the
    /// fallibility.
    pub fn graph(&mut self) -> Result<&ProjectGraph> {
        if self.graph.len() == 0 {
            self.populate_graph()?;
        }

        Ok(&self.graph)
    }

    fn populate_graph(&mut self) -> Result<()> {
        // Start by auto-detecting everything in the Git index.

        let index = self.repo.index()?;

        for entry in index.iter() {
            let (dirname, basename) = RepoPath::new(&entry.path).split_basename();
            let maybe_proj = if basename.as_ref() == b"Cargo.toml" {
                Project::new_from_prefix::<crate::projmeta::cargo::CargoMetadata>(
                    &self,
                    self.projects.len(),
                    dirname,
                )?
            } else {
                None
            };

            if let Some(p) = maybe_proj {
                println!("got one {:?}", p);
                self.projects.push(p);
            }
        }

        // Populate the graph.
        for p in &self.projects {
            self.graph.add_project(&p);
        }

        Ok(())
    }
}

/// A borrowed reference to a pathname as understood by the backing repository.
///
/// In git, such a path is a byte array. The directory separator is always "/".
/// The bytes are often convertible to UTF-8, but not always.
#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RepoPath([u8]);

impl std::convert::AsRef<RepoPath> for [u8] {
    fn as_ref(&self) -> &RepoPath {
        unsafe { &*(self.as_ref() as *const [_] as *const RepoPath) }
    }
}

impl std::convert::AsRef<[u8]> for RepoPath {
    fn as_ref(&self) -> &[u8] {
        unsafe { &*(self.0.as_ref() as *const [u8]) }
    }
}

impl RepoPath {
    fn new(p: &[u8]) -> &Self {
        p.as_ref()
    }

    /// Split a path into a directory name and a file basename.
    ///
    /// Returns `(dirname, basename)`. The dirname will be empty if the path
    /// contains no separator. Otherwise, it will end with the path separator.
    /// It is always true that `self = concat(dirname, basename)`.
    pub fn split_basename(&self) -> (&RepoPath, &RepoPath) {
        // Have to index the dirname manually since split() and friends don't
        // include the separating items, which we want.
        let basename = self.0.rsplit(|c| *c == b'/').next().unwrap();
        let ndir = self.0.len() - basename.len();
        return (&self.0[..ndir].as_ref(), basename.as_ref());
    }

    /// Convert the repository path into an OS path
    pub fn as_path(&self) -> &Path {
        bytes2path(&self.0)
    }
}

// Copied from git2-rs src/util.rs
#[cfg(unix)]
fn bytes2path(b: &[u8]) -> &Path {
    use std::{ffi::OsStr, os::unix::prelude::*};
    Path::new(OsStr::from_bytes(b))
}
#[cfg(windows)]
fn bytes2path(b: &[u8]) -> &Path {
    use std::str;
    Path::new(str::from_utf8(b).unwrap())
}
