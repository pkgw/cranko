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

/// Opaque type representing a commit in the repository.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitId(git2::Oid);

impl std::fmt::Display for CommitId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

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

    /// Get the name of the `rc`-type branch.
    pub fn upstream_rc_name(&self) -> &str {
        &self.upstream_rc_name
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

    fn try_get_rc_commit(&self) -> Result<Option<git2::Commit>> {
        let rc_ref = match self.repo.resolve_reference_from_short_name(&format!(
            "{}/{}",
            self.upstream_name, self.upstream_rc_name
        )) {
            Ok(r) => r,
            Err(e) => {
                return if e.code() == git2::ErrorCode::NotFound {
                    // No `rc` branch in the upstream, which is OK
                    Ok(None)
                } else {
                    Err(e.into())
                };
            }
        };

        Ok(Some(rc_ref.peel_to_commit()?))
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

        let mut info = SerializedReleaseCommitInfo::default();

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

    /// Look at the commits between HEAD and the latest release and analyze
    /// their diffs to categorize which commits affect which projects.
    ///
    /// TODO: say that a subproject was modified but not released in the most
    /// recent release commit -- something that we want to allow for
    /// practicality. For that project we will need to reach farther back in the
    /// history than the tip of `release`, which will force this algorithm to
    /// become a lot more complicated.
    pub fn analyze_history_to_release(
        &self,
        matchers: &[&PathMatcher],
    ) -> Result<Vec<Vec<CommitId>>> {
        // Set up to walk the history.

        let mut walk = self.repo.revwalk()?;

        walk.push_head()?;

        if let Some(release_commit) = self.try_get_release_commit()? {
            walk.hide(release_commit.id())?;
        }

        // Set up our results table.

        let mut hit_buf = vec![false; matchers.len()];
        let mut matches = vec![Vec::new(); matchers.len()];

        // Do the walk!

        let mut trees = lru::LruCache::new(3);
        let mut dopts = git2::DiffOptions::new();
        dopts.include_typechange(true);

        for maybe_oid in walk {
            // Get the two relevant trees and compute their diff. We have to
            // jump through some hoops to support the root commit (with no
            // parents) but it's not really that bad. We also have to pop() the
            // trees out of the LRU because get() holds a mutable reference to
            // the cache, which prevents us from looking at two trees
            // simultaneously.

            let oid = maybe_oid?;
            let commit = self.repo.find_commit(oid)?;
            let ctid = commit.tree_id();
            let cur_tree = match trees.pop(&ctid) {
                Some(t) => t,
                None => self.repo.find_tree(ctid)?,
            };

            let (maybe_ptid, maybe_parent_tree) = if commit.parent_count() == 0 {
                (None, None) // this is the first commit in the history!
            } else {
                let parent = commit.parent(0)?;
                let ptid = parent.tree_id();
                let parent_tree = match trees.pop(&ptid) {
                    Some(t) => t,
                    None => self.repo.find_tree(ptid)?,
                };
                (Some(ptid), Some(parent_tree))
            };

            let diff = self.repo.diff_tree_to_tree(
                maybe_parent_tree.as_ref(),
                Some(&cur_tree),
                Some(&mut dopts),
            )?;

            trees.put(ctid, cur_tree);
            if let (Some(ptid), Some(pt)) = (maybe_ptid, maybe_parent_tree) {
                trees.put(ptid, pt);
            }

            // Examine the diff and see what file paths, and therefore which
            // projects, are affected.

            for flag in &mut hit_buf {
                *flag = false;
            }

            for delta in diff.deltas() {
                // there's presumably a cleaner way to do this?
                if let Some(old_path_bytes) = delta.old_file().path_bytes() {
                    let old_path = RepoPath::new(old_path_bytes);
                    for (idx, matcher) in matchers.iter().enumerate() {
                        if matcher.repo_path_matches(old_path) {
                            hit_buf[idx] = true;
                        }
                    }
                }

                if let Some(new_path_bytes) = delta.new_file().path_bytes() {
                    let new_path = RepoPath::new(new_path_bytes);
                    for (idx, matcher) in matchers.iter().enumerate() {
                        if matcher.repo_path_matches(new_path) {
                            hit_buf[idx] = true;
                        }
                    }
                }
            }

            for (idx, commit_list) in matches.iter_mut().enumerate() {
                if hit_buf[idx] {
                    commit_list.push(CommitId(oid.clone()));
                }
            }
        }

        Ok(matches)
    }

    /// Get the brief message associated with a commit.
    pub fn get_commit_summary(&self, cid: CommitId) -> Result<String> {
        let commit = self.repo.find_commit(cid.0)?;

        if let Some(s) = commit.summary() {
            Ok(s.to_owned())
        } else {
            Ok(format!("[commit {0}: non-Unicode summary]", cid.0))
        }
    }

    /// Examine a project's state in the working directory and report whether it
    /// is properly staged for a release request.
    ///
    /// Returns None if there's nothing wrong but this project doesn't seem to
    /// have been staged for release.
    ///
    /// Modified changelog files are register with the *changes* listing.
    pub fn scan_rc_info(
        &self,
        proj: &Project,
        changes: &mut ChangeList,
    ) -> Result<Option<RcProjectInfo>> {
        let mut saw_changelog = false;

        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        opts.include_ignored(true);

        for entry in self.repo.statuses(Some(&mut opts))?.iter() {
            let path = RepoPath::new(entry.path_bytes());
            if !proj.repo_paths.repo_path_matches(path) {
                continue;
            }

            let status = entry.status();

            if proj.changelog.is_changelog_path_for(proj, path) {
                if status.is_conflicted() {
                    return Err(Error::DirtyRepository(path.escaped()));
                } else if status.is_index_new() || status.is_index_modified() {
                    changes.add_path(path);
                    saw_changelog = true;
                } // TODO: handle/complain about some other statuses
            } else {
                if status.is_ignored() || status.is_wt_new() || status == git2::Status::CURRENT {
                } else {
                    return Err(Error::DirtyRepository(path.escaped()));
                }
            }
        }

        if saw_changelog {
            Ok(Some(proj.changelog.scan_rc_info(proj, self)?))
        } else {
            Ok(None)
        }
    }

    /// Make a commit merging changelog modifications and and release request
    /// information into the rc branch.
    pub fn make_rc_commit(
        &mut self,
        rcinfo: Vec<RcProjectInfo>,
        changes: &ChangeList,
    ) -> Result<()> {
        // Gather useful info.

        let maybe_rc_commit = self.try_get_rc_commit()?;
        let head_ref = self.repo.head()?;
        let head_commit = head_ref.peel_to_commit()?;
        let sig = self.get_signature()?;
        let local_ref_name = format!("refs/heads/{}", self.upstream_rc_name);

        // Set up the release request info. This will be serialized into the
        // commit message.

        let mut info = SerializedRcCommitInfo::default();
        info.projects = rcinfo;

        let message = format!(
            "Release request commit created with Cranko.

+++ cranko-rc-info-v1
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

        // Create the merged rc commit and save it under the
        // local_ref_name.

        let commit = |parents: &[&git2::Commit]| -> Result<git2::Oid> {
            self.repo
                .reference(&local_ref_name, parents[0].id(), true, "update rc")?;
            Ok(self.repo.commit(
                Some(&local_ref_name), // update_ref
                &sig,                  // author
                &sig,                  // committer
                &message,
                &tree,
                parents,
            )?)
        };

        if let Some(release_commit) = maybe_rc_commit {
            commit(&[&release_commit, &head_commit])?;
        } else {
            commit(&[&head_commit])?;
        };

        // Unlike the release commit workflow, we don't switch to the new
        // branch.

        Ok(())
    }

    /// Get information about a `rc` release request from the HEAD commit.
    pub fn parse_rc_info_from_head(&self) -> Result<RcCommitInfo> {
        let head_ref = self.repo.head()?;
        let head_commit = head_ref.peel_to_commit()?;
        let msg = head_commit
            .message()
            .ok_or_else(|| Error::NotUnicodeError)?;

        let mut data = String::new();
        let mut in_body = false;

        for line in msg.lines() {
            if in_body {
                if line == "+++" {
                    in_body = false;
                    break;
                } else {
                    data.push_str(line);
                    data.push('\n');
                }
            } else if line.starts_with("+++ cranko-rc-info-v1") {
                in_body = true;
            }
        }

        if in_body {
            println!("unterminated RC info body; trying to proceed anyway");
        }

        if data.len() == 0 {
            return Err(Error::InvalidCommitMessageFormat);
        }

        let srci: SerializedRcCommitInfo = toml::from_str(&data)?;

        Ok(RcCommitInfo {
            committish: Some(CommitId(head_commit.id())),
            projects: srci.projects,
        })
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
    pub committish: Option<CommitId>,

    /// A list of projects and their release information as of this commit. This
    /// list includes every tracked project in this commit. Not all of those
    /// projects necessarily were released with this commit, if they were
    /// unchanged from a previous release commit.
    pub projects: Vec<ReleasedProjectInfo>,
}

impl ReleaseCommitInfo {
    /// Attempt to find info for a prior release of the named project.
    ///
    /// Information may be missing if the project was only added to the
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
struct SerializedReleaseCommitInfo {
    pub projects: Vec<ReleasedProjectInfo>,
}

/// Serializable state information about a single project in a release commit.
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

/// Information about the projects in the repository corresponding to an "rc"
/// commit where the user has requested that one or more of the projects be
/// released.
#[derive(Debug, Default)]
pub struct RcCommitInfo {
    /// The Git commit-ish that this object describes.
    pub committish: Option<CommitId>,

    /// A list of projects and their "rc" information as of this commit. This
    /// should contain at least one project, but doesn't necessarily include
    /// every project in the repo.
    pub projects: Vec<RcProjectInfo>,
}

impl RcCommitInfo {
    /// Attempt to find info for a release request for the specified project.
    pub fn lookup_project(&self, proj: &Project) -> Option<&RcProjectInfo> {
        // TODO: redundant with ReleaseCommitInfo::lookup_project()

        for rci in &self.projects {
            if rci.qnames == *proj.qualified_names() {
                return Some(rci);
            }
        }

        // TODO: any more sophisticated search to try?
        None
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct SerializedRcCommitInfo {
    pub projects: Vec<RcProjectInfo>,
}

/// Serializable state information about a single project with a proposed
/// release in an `rc` commit.
#[derive(Debug, Deserialize, Serialize)]
pub struct RcProjectInfo {
    /// The qualified names of this project, equivalent to the same-named
    /// property of the Project struct.
    pub qnames: Vec<String>,

    /// The kind of version bump requested by the user.
    pub bump_spec: String,
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

/// A filter that matches paths inside the repository and/or working directory.
///
/// We're not trying to get fully general here, but there is a common use case
/// that we need to support. A monorepo might contain a toplevel project, rooted
/// at the repo base, plus one or more subprojects in some kind of
/// subdirectories. For the toplevel project, we need to express a match for a
/// file anywhere in the repo *except* ones that match any of the subprojects.
#[derive(Debug)]
pub struct PathMatcher {
    terms: Vec<PathMatcherTerm>,
}

impl PathMatcher {
    /// Create a new matcher that includes only files in the specified repopath
    /// prefix.
    pub fn new_include(p: RepoPathBuf) -> Self {
        let terms = vec![PathMatcherTerm::Include(p)];
        PathMatcher { terms }
    }

    /// Modify this matcher to exclude any paths that *other* would include.
    ///
    /// This whole framework could surely be a lot more efficient, but unless
    /// your repo has 1000 projects it's just not going to matter, I think.
    pub fn make_disjoint(&mut self, other: &PathMatcher) -> &mut Self {
        let mut new_terms = Vec::new();

        for other_term in &other.terms {
            if let PathMatcherTerm::Include(ref other_pfx) = other_term {
                for term in &self.terms {
                    if let PathMatcherTerm::Include(ref pfx) = term {
                        // We only need to exclude terms in the other matcher
                        // that are more specific than ours.
                        if other_pfx.starts_with(pfx) {
                            new_terms.push(PathMatcherTerm::Exclude(other_pfx.clone()));
                        }
                    }
                }
            }
        }

        new_terms.append(&mut self.terms);
        self.terms = new_terms;
        self
    }

    /// Test whether a repo-path matches.
    pub fn repo_path_matches(&self, p: &RepoPath) -> bool {
        for term in &self.terms {
            match term {
                PathMatcherTerm::Include(pfx) => {
                    if p.starts_with(pfx) {
                        return true;
                    }
                }

                PathMatcherTerm::Exclude(pfx) => {
                    if p.starts_with(pfx) {
                        return false;
                    }
                }
            }
        }

        false
    }
}

#[derive(Debug)]
enum PathMatcherTerm {
    /// Include paths prefixed by the value.
    Include(RepoPathBuf),

    /// Exclude paths prefixed by the value.
    Exclude(RepoPathBuf),
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

    /// Return true if this path starts with the argument.
    pub fn starts_with<P: AsRef<[u8]>>(&self, other: P) -> bool {
        let other = other.as_ref();
        let sn = self.len();
        let on = other.len();

        if sn < on {
            false
        } else {
            &self.0[..on] == other
        }
    }

    /// Return true if this path ends with the argument.
    pub fn ends_with<P: AsRef<[u8]>>(&self, other: P) -> bool {
        let other = other.as_ref();
        let sn = self.len();
        let on = other.len();

        if sn < on {
            false
        } else {
            &self.0[(sn - on)..] == other
        }
    }
}

impl git2::IntoCString for &RepoPath {
    fn into_c_string(self) -> std::result::Result<std::ffi::CString, git2::Error> {
        self.0.into_c_string()
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
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RepoPathBuf(Vec<u8>);

impl std::convert::AsRef<RepoPath> for RepoPathBuf {
    fn as_ref(&self) -> &RepoPath {
        RepoPath::new(&self.0[..])
    }
}

impl std::convert::AsRef<[u8]> for RepoPathBuf {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
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
