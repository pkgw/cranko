// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State of the backing version control repository.

use std::path::{Path, PathBuf};

use crate::errors::{Error, Result};

/// Information about the backing version control repository.
pub struct Repository {
    /// The underlying `git2` repository object.
    repo: git2::Repository,

    /// The name of the "upstream" remote that hosts the `rc` and `release`
    /// branches of record.
    upstream_name: String,

    /// The name of the `rc`-type branch in the upstream remote. This is
    /// optional since we want to be able to run successfully even if the
    /// upstream isn't fully configured.
    upstream_rc_name: Option<String>,

    /// The name of the `release`-type branch in the upstream remote. Also
    /// optional.
    upstream_release_name: Option<String>,
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

        // Guess the name of the upstream remote. If there's only one remote, we
        // use it; if there are multiple and one is "origin", we use it.
        // Otherwise, we error out. TODO: make this configurable, add more
        // heuristics. Note that this config item should not be stored in the
        // repo since it can be unique to each checkout. (What *could* be stored
        // in the repo would be a list of URLs corresponding to the official
        // upstream, and we could see if any of the remotes have one of those
        // URLs.)

        let mut upstream_name = None;
        let mut n_remotes = 0;

        for remote_name in &repo.remotes()? {
            // `None` happens if a remote name is not valid UTF8. At the moment
            // I can't be bothered to properly handle that.
            if let Some(remote_name) = remote_name {
                n_remotes += 1;

                if upstream_name.is_none() || remote_name == "origin" {
                    upstream_name = Some(remote_name.to_owned());
                }
            }
        }

        if upstream_name.is_none() || (n_remotes > 1 && upstream_name.as_deref() != Some("origin"))
        {
            return Err(Error::NoUpstreamRemote);
        }

        let upstream_name = upstream_name.unwrap();

        // Now that we've got that, check for the upstream `rc` and `release`
        // branches. This could/should also be configurable. Note that this
        // configuration could be stored in the repository since every checkout
        // should be talking about the same upstream.

        let mut upstream_rc_name = None;
        let mut upstream_release_name = None;
        let n_uname = upstream_name.len();

        for maybe_branch in repo.branches(Some(git2::BranchType::Remote))? {
            if let Ok((branch, _type)) = maybe_branch {
                if let Some(bname) = branch.name()? {
                    let n_bname = bname.len();

                    if n_bname == n_uname + 3
                        && bname.starts_with(&upstream_name)
                        && bname.ends_with("/rc")
                    {
                        upstream_rc_name = Some(bname.to_owned());
                    }

                    if n_bname == n_uname + 8
                        && bname.starts_with(&upstream_name)
                        && bname.ends_with("/release")
                    {
                        upstream_release_name = Some(bname.to_owned());
                    }
                }
            }
        }

        // All set up.

        Ok(Repository {
            repo,
            upstream_name,
            upstream_rc_name,
            upstream_release_name,
        })
    }

    /// Resolve a `RepoPath` repository path to a filesystem path in the working
    /// directory.
    pub fn resolve_workdir(&self, p: &RepoPath) -> PathBuf {
        let mut fullpath = self.repo.workdir().unwrap().to_owned();
        fullpath.push(p.as_path());
        fullpath
    }

    /// Scan the paths in the repository index.
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

    /// Check that the repository is clean. We allow untracked and ignored files
    /// but otherwise don't want any modifications, etc.
    pub fn check_dirty(&self) -> Result<()> {
        // Default options are what we want.
        let mut opts = git2::StatusOptions::new();

        for entry in self.repo.statuses(Some(&mut opts))?.iter() {
            // Is this correct / sufficient?
            if entry.status() != git2::Status::CURRENT {
                return Err(Error::DirtyRepository(escape_pathlike(entry.path_bytes())));
            }
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

/// Convert an arbitrary byte slice to something printable.
///
/// If the bytes can be interpreted as UTF-8, their Unicode stringification will
/// be returned. Otherwise, bytes that aren't printable ASCII will be
/// backslash-escaped, and the whole string will be wrapped in double quotes.
///
/// **Note**: we should probably only do a direct conversion if it's printable
/// ASCII without whitespaces, etc. To be refined.
pub fn escape_pathlike(b: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(b) {
        s.to_owned()
    } else {
        let mut buf = vec![b'\"'];
        buf.extend(b.iter().map(|c| std::ascii::escape_default(*c)).flatten());
        buf.push(b'\"');
        String::from_utf8(buf).unwrap()
    }
}
