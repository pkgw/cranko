// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State of the backing version control repository.

//use dynfmt::{Format, SimpleCurlyFormat};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{
    errors::{Error, Result},
    graph::ProjectGraph,
    project::Project,
};

/// Information about the backing version control repository.
pub struct Repository {
    /// The underlying `git2` repository object.
    repo: git2::Repository,

    /// The name of the "upstream" remote that hosts the `rc` and `release`
    /// branches of record.
    upstream_name: String,

    /// The name of the `rc`-type branch in the upstream remote. The branch
    /// itself might not exist, if the upstream repo is just being initialized.
    upstream_rc_name: String,

    /// The name of the `release`-type branch in the upstream remote. As with `rc`,
    /// the branch itself might not exist.
    upstream_release_name: String,

    /// The format specification to use for release tag names, as understood by
    /// the `SimpleCurlyFormat` of the `dynfmt` crate.
    release_tag_name_format: String,
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

        let upstream_rc_name = "rc".to_owned();
        let upstream_release_name = "release".to_owned();

        // Release tag name format. Should also become configurable.

        let release_tag_name_format = "{project_slug}@{version}".to_owned();

        // All set up.

        Ok(Repository {
            repo,
            upstream_name,
            upstream_rc_name,
            upstream_release_name,
            release_tag_name_format,
        })
    }

    /// Resolve a `RepoPath` repository path to a filesystem path in the working
    /// directory.
    pub fn resolve_workdir(&self, p: &RepoPath) -> PathBuf {
        let mut fullpath = self.repo.workdir().unwrap().to_owned();
        fullpath.push(p.as_path());
        fullpath
    }

    /// Convert a filesystem path pointing inside the working directory into a
    /// RepoPathBuf.
    ///
    /// Some external tools (e.g. `cargo metadata`) make it so that it is useful
    /// to be able to do this reverse conversion.
    pub fn convert_path<P: AsRef<Path>>(&self, p: P) -> Result<RepoPathBuf> {
        let c_root = self.repo.workdir().unwrap().canonicalize()?;
        let c_p = p.as_ref().canonicalize()?;
        let rel = c_p
            .strip_prefix(&c_root)
            .map_err(|_| Error::OutsideOfRepository(c_p.display().to_string()))?;
        RepoPathBuf::from_path(rel)
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

    /// Get information about the state of the projects in the repository as
    /// of the latest release commit.
    pub fn get_latest_release_info(&self) -> Result<ReleaseCommitInfo> {
        if let Some(_c) = self.try_get_release_commit()? {
            unimplemented!("get info from commit!");
        } else {
            Ok(ReleaseCommitInfo::default())
        }
    }

    fn get_signature(&self) -> Result<git2::Signature> {
        Ok(git2::Signature::now("cranko", "cranko@devnull")?)
    }

    fn try_get_release_commit(&self) -> Result<Option<git2::Commit>> {
        let release_ref = match self.repo.resolve_reference_from_short_name(&format!(
            "{}/{}",
            self.upstream_name, self.upstream_release_name
        )) {
            Ok(r) => r,
            Err(e) => {
                return if e.code() == git2::ErrorCode::NotFound {
                    // No `release` branch in the upstream, which is OK
                    Ok(None)
                } else {
                    Err(e.into())
                };
            }
        };

        Ok(Some(release_ref.peel_to_commit()?))
    }

    /// Make a commit merging the current workdir state into the release branch.
    pub fn make_release_commit(
        &mut self,
        graph: &ProjectGraph,
        changes: &ChangeList,
    ) -> Result<()> {
        // Gather useful info.

        let maybe_release_commit = self.try_get_release_commit()?;
        let head_ref = self.repo.head()?;
        let head_commit = head_ref.peel_to_commit()?;
        let sig = self.get_signature()?;
        let local_ref_name = format!("refs/heads/{}", self.upstream_release_name);

        // Set up the project release info. This will be serialized into the
        // commit message. (In principle, we could attempt to extract this
        // information from the Git Tree associated with the release commit, but
        // not only would that be harder to implement, it would introduce all
        // sorts of fragility into the system as data formats change. Better to
        // just save the data as data.)

        let mut info = SerializedCommitInfo::default();

        for proj in graph.toposort()? {
            info.projects.push(ReleasedProjectInfo {
                qnames: proj.qualified_names().clone(),
                version: proj.version.to_string(),
                age: proj.version_age,
            });
        }

        let message = format!(
            "Release commit created with Cranko.

+++ cranko-release-info-v1
{}
+++
",
            toml::to_string(&info)?
        );

        // Create and save a new Tree containing the working-tree changes made
        // during the rewrite process.

        let tree_oid = {
            let mut index = self.repo.index()?;

            for p in &changes.paths {
                index.add_path(p.as_path())?;
            }

            index.write_tree()?
        };
        let tree = self.repo.find_tree(tree_oid)?;

        // Create the merged release commit and save it under the
        // local_ref_name.

        let commit = |parents: &[&git2::Commit]| -> Result<git2::Oid> {
            self.repo
                .reference(&local_ref_name, parents[0].id(), true, "update release")?;
            Ok(self.repo.commit(
                Some(&local_ref_name), // update_ref
                &sig,                  // author
                &sig,                  // committer
                &message,
                &tree,
                parents,
            )?)
        };

        let commit_id = if let Some(release_commit) = maybe_release_commit {
            commit(&[&release_commit, &head_commit])?
        } else {
            commit(&[&head_commit])?
        };

        // Switch the working directory to be the checkout of our new merge
        // commit. By construction, nothing on the filesystem should actually
        // change.

        self.repo.set_head(&local_ref_name)?;
        self.repo.reset(
            self.repo.find_commit(commit_id)?.as_object(),
            git2::ResetType::Mixed,
            None,
        )?;

        // Phew, all done!

        Ok(())
    }
}

/// Information about the state of the projects in the repository corresponding
/// to a "release" commit where all of the projects have been assigned version
/// numbers, and the commit should have made it out into the wild only if all of
/// the CI tests passed.
#[derive(Debug, Default)]
pub struct ReleaseCommitInfo {
    /// The Git commit-ish that this object describes. May be None when there is
    /// no upstream `release` branch, in which case this struct will contain no
    /// genuine information.
    pub committish: Option<git2::Oid>,

    /// A list of projects and their release information as of this commit. This
    /// list includes every tracked project in this commit. Not all of those
    /// projects necessarily were released with this commit, if they were
    /// unchanged from a previous release commit.
    pub projects: Vec<ReleasedProjectInfo>,
}

impl ReleaseCommitInfo {
    /// Attempt to find info for a prior release of the named project.
    ///
    /// Information may be missing of the project was only added to the
    /// repository after this information was recorded.
    pub fn lookup_project(&self, proj: &Project) -> Option<&ReleasedProjectInfo> {
        for rpi in &self.projects {
            if rpi.qnames == *proj.qualified_names() {
                return Some(rpi);
            }
        }

        // TODO: any more sophisticated search to try?
        None
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct SerializedCommitInfo {
    pub projects: Vec<ReleasedProjectInfo>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReleasedProjectInfo {
    /// The qualified names of this project, equivalent to the same-named
    /// property of the Project struct.
    pub qnames: Vec<String>,

    /// The version of the project in this commit, as text.
    pub version: String,

    /// The number of consecutive release commits for which this project
    /// has had the assigned version string. If zero, that means that the
    /// specified version was first released with this commit.
    pub age: usize,
}

/// A data structure recording changes made when rewriting files
/// in the repository.
#[derive(Debug, Default)]
pub struct ChangeList {
    paths: Vec<RepoPathBuf>,
}

impl ChangeList {
    /// Mark the file at this path as having been updated.
    pub fn add_path(&mut self, p: &RepoPath) {
        self.paths.push(p.to_owned());
    }
}

// Below we have helpers for trying to deal with git's paths properly, on the
// off-chance that they contain invalid UTF-8 and the like.

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

    /// Compute a user-displayable escaped version of this path.
    pub fn escaped(&self) -> String {
        escape_pathlike(&self.0)
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

    /// Create a RepoPathBuf from a Path-like. It is assumed that the path is
    /// relative to the repository working directory root and doesn't have any
    /// funny business like ".." in it.
    #[cfg(unix)]
    fn from_path<P: AsRef<Path>>(p: P) -> Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        Ok(Self::new(p.as_ref().as_os_str().as_bytes()))
    }

    /// Create a RepoPathBuf from a Path-like. It is assumed that the path is
    /// relative to the repository working directory root and doesn't have any
    /// funny business like ".." in it.
    #[cfg(windows)]
    fn from_path<P: AsRef<Path>>(p: P) -> Result<Self> {
        let mut first = true;
        let mut b = Vec::new();

        for cmpt in p.as_ref().components() {
            if first {
                first = false;
            } else {
                b.push(b'/');
            }

            if let std::path::Component::Normal(c) = cmpt {
                b.extend(c.to_str().unwrap().as_bytes());
            } else {
                return Err(Error::OutsideOfRepository(format!(
                    "path with unexpected components: {}",
                    p.as_ref().display()
                )));
            }
        }

        Ok(RepoPathBuf(b))
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
