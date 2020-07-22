// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State of the backing version control repository.

use std::path::{Path, PathBuf};

use crate::errors::{Error, Result};

/// Information about the backing version control repository.
pub struct Repository {
    /// The underlying `git2` repository object.
    repo: git2::Repository,
}

impl Repository {
    /// Open the repository using standard environmental cues.
    ///
    /// Initialization may fail if the process is not running inside a Git
    /// repository and the necessary Git environment variables are missing, if
    /// the repository is "bare" (has no working directory), if there is some
    /// data corruption issue, etc.
    pub fn open_from_env() -> Result<Repository> {
        let repo = git2::Repository::open_from_env()?;

        if repo.is_bare() {
            return Err(Error::BareRepository);
        }

        Ok(Repository { repo })
    }

    /// Resolve a `RepoPath` repository path to a filesystem path in the working
    /// directory.
    pub fn resolve_workdir(&self, p: &RepoPath) -> PathBuf {
        let mut fullpath = self.repo.workdir().unwrap().to_owned();
        fullpath.push(p.as_path());
        fullpath
    }

    // Scan the paths in the repository index.
    pub fn scan_paths<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(&RepoPath) -> (),
    {
        // We have to use a callback here since the IndexEntries iter holds a
        // ref to the index, which wherefore has to be immovable (pinned) during
        // the iteration process.
        let index = self.repo.index()?;

        for entry in index.iter() {
            f(RepoPath::new(&entry.path));
        }

        Ok(())
    }
}

/// A borrowed reference to a pathname as understood by the backing repository.
///
/// In git, such a path is a byte array. The directory separator is always "/".
/// The bytes are often convertible to UTF-8, but not always. (These are the
/// same semantics as Unix paths.)
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

    /// Get the length of the path, in bytes
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Convert the repository path into an OS path.
    pub fn as_path(&self) -> &Path {
        bytes2path(&self.0)
    }

    /// Convert this borrowed reference into an owned copy.
    pub fn to_owned(&self) -> RepoPathBuf {
        RepoPathBuf::new(&self.0[..])
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

/// An owned reference to a pathname as understood by the backing repository.
#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RepoPathBuf(Vec<u8>);

impl std::convert::AsRef<RepoPath> for RepoPathBuf {
    fn as_ref(&self) -> &RepoPath {
        RepoPath::new(&self.0[..])
    }
}

impl RepoPathBuf {
    pub fn new(b: &[u8]) -> Self {
        RepoPathBuf(b.to_vec())
    }

    pub fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }
}

impl std::ops::Deref for RepoPathBuf {
    type Target = RepoPath;

    fn deref(&self) -> &RepoPath {
        RepoPath::new(&self.0[..])
    }
}
