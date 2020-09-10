// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State of the backing version control repository.

use anyhow::anyhow;
use dynfmt::{Format, SimpleCurlyFormat};
use log::info;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};
use thiserror::Error as ThisError;

use crate::{
    errors::{Error, OldError, Result},
    graph::ProjectGraph,
    project::Project,
    version::Version,
};

/// Opaque type representing a commit in the repository.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitId(git2::Oid);

impl std::fmt::Display for CommitId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, ThisError)]
#[error("cannot operate on a bare repository")]
pub struct BareRepositoryError;

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
    ///
    /// If the repository is "bare", an error downcastable into
    /// BareRepositoryError will be returned.
    pub fn open_from_env() -> Result<Repository> {
        let repo = git2::Repository::open_from_env()?;

        if repo.is_bare() {
            return Err(BareRepositoryError.into());
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
            return Err(OldError::NoUpstreamRemote.into());
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

    /// Get the name of the `release`-type branch.
    pub fn upstream_release_name(&self) -> &str {
        &self.upstream_release_name
    }

    /// Get the URL of the upstream repository.
    pub fn upstream_url(&self) -> Result<String> {
        let upstream = self.repo.find_remote(&self.upstream_name)?;
        Ok(upstream
            .url()
            .ok_or_else(|| {
                anyhow!(
                    "URL of upstream remote {} not parseable as Unicode",
                    self.upstream_name
                )
            })?
            .to_owned())
    }

    /// Get the name of the currently active branch, if there is one.
    ///
    /// There might not be such a branch if the repository is in a "detached
    /// HEAD" state, for instance.
    pub fn current_branch_name(&self) -> Result<Option<String>> {
        let head_ref = self.repo.head()?;

        Ok(if !head_ref.is_branch() {
            None
        } else {
            Some(
                head_ref
                    .shorthand()
                    .ok_or_else(|| anyhow!("current branch name not Unicode"))?
                    .to_owned(),
            )
        })
    }

    /// Parse a textual reference to a commit within the repository.
    pub fn parse_commit_ref<T: AsRef<str>>(&self, text: T) -> Result<CommitRef> {
        let text = text.as_ref();

        if let Ok(id) = text.parse() {
            Ok(CommitRef::Id(CommitId(id)))
        } else if text.starts_with("thiscommit:") {
            Ok(CommitRef::ThisCommit {
                salt: text[11..].to_owned(),
            })
        } else {
            Err(OldError::InvalidCommitReference(text.to_owned()).into())
        }
    }

    /// Resolve a commit reference to a specific commit ID
    pub fn resolve_commit_ref(
        &self,
        cref: &CommitRef,
        ref_source_path: &RepoPath,
    ) -> Result<CommitId> {
        let cid = match cref {
            CommitRef::Id(id) => id.clone(),
            CommitRef::ThisCommit { ref salt } => lookup_this(self, salt, ref_source_path)?,
        };

        // Double-check that the ID actually resolves to a commit.
        self.repo.find_commit(cid.0)?;
        return Ok(cid);

        fn lookup_this(
            repo: &Repository,
            salt: &str,
            ref_source_path: &RepoPath,
        ) -> Result<CommitId> {
            let file = File::open(repo.resolve_workdir(ref_source_path))?;
            let reader = BufReader::new(file);
            let mut line_no = 1; // blames start at line 1.
            let mut found_it = false;

            for maybe_line in reader.lines() {
                let line = maybe_line?;
                if line.contains(salt) {
                    found_it = true;
                    break;
                }

                line_no += 1;
            }

            if !found_it {
                return Err(anyhow!(
                    "commit-ref key `{}` not found in contents of file {}",
                    salt,
                    ref_source_path.escaped(),
                ));
            }

            let blame = repo.repo.blame_file(ref_source_path.as_path(), None)?;
            let hunk = blame.get_line(line_no).ok_or_else(|| {
                // TODO: this happens if the line in question hasn't yet been
                // committed. Need to figure out how to handle that
                // circumstance.
                anyhow!(
                    "commit-ref key `{}` found in non-existent line {} of file {}??",
                    salt,
                    line_no,
                    ref_source_path.escaped()
                )
            })?;

            Ok(CommitId(hunk.final_commit_id()))
        }
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
            .map_err(|_| OldError::OutsideOfRepository(c_p.display().to_string()))?;
        RepoPathBuf::from_path(rel)
    }

    /// Scan the paths in the repository index.
    pub fn scan_paths<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(&RepoPath) -> (),
    {
        // We have to use a callback here since the IndexEntries iter holds a
        // ref to the index, which therefore has to be immovable (pinned) during
        // the iteration process.
        let index = self.repo.index()?;

        for entry in index.iter() {
            f(RepoPath::new(&entry.path));
        }

        Ok(())
    }

    /// Check if the working tree is clean. Returns None if there are no
    /// modifications and Some(escaped_path) if there are any. (The escaped_path
    /// will be the first one encountered in the check, an essentially arbitrary
    /// selection.) Modifications to any of the paths matched by `ok_matchers`
    /// are allowed.
    pub fn check_if_dirty(&self, ok_matchers: &[PathMatcher]) -> Result<Option<String>> {
        // Default options are what we want.
        let mut opts = git2::StatusOptions::new();

        for entry in self.repo.statuses(Some(&mut opts))?.iter() {
            // Is this correct / sufficient?
            if entry.status() != git2::Status::CURRENT {
                let repo_path = RepoPath::new(entry.path_bytes());
                let mut is_ok = false;

                for matcher in ok_matchers {
                    if matcher.repo_path_matches(repo_path) {
                        is_ok = true;
                        break;
                    }
                }

                if !is_ok {
                    return Ok(Some(repo_path.escaped()));
                }
            }
        }

        Ok(None)
    }

    /// Get the binary content of the file at the specified path, at the time of
    /// the specified commit. If the path did not exist, `Ok(None)` is returned.
    pub fn get_file_at_commit(&self, cid: &CommitId, path: &RepoPath) -> Result<Option<Vec<u8>>> {
        let commit = self.repo.find_commit(cid.0)?;
        let tree = commit.tree()?;
        let entry = match tree.get_path(path.as_path()) {
            Ok(e) => e,
            Err(e) => {
                return if e.code() == git2::ErrorCode::NotFound {
                    Ok(None)
                } else {
                    Err(e.into())
                };
            }
        };
        let object = entry.to_object(&self.repo)?;
        let blob = object.as_blob().ok_or_else(|| {
            anyhow!(
                "path `{}` should correspond to a Git blob but does not",
                path.escaped(),
            )
        })?;

        Ok(Some(blob.content().to_owned()))
    }

    /// Get information about the state of the projects in the repository as
    /// of the latest release commit.
    pub fn get_latest_release_info(&self) -> Result<ReleaseCommitInfo> {
        if let Some(c) = self.try_get_release_commit()? {
            Ok(self.parse_release_info_from_commit(&c)?)
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

    /// Make a commit merging the current index state into the release branch.
    pub fn make_release_commit(&mut self, graph: &ProjectGraph) -> Result<()> {
        // Gather useful info.

        let rel_info = self.get_latest_release_info()?;
        let head_ref = self.repo.head()?;
        let head_commit = head_ref.peel_to_commit()?;
        let sig = self.get_signature()?;
        let local_ref_name = format!("refs/heads/{}", self.upstream_release_name);

        // Set up the project release info. This will be serialized into the
        // commit message. (In principle, other commands could attempt to
        // extract this information from the Git Tree associated with the
        // release commit, but not only would that be harder to implement, it
        // would introduce all sorts of fragility into the system as data
        // formats change. Better to just save the data as data.)

        let mut info = SerializedReleaseCommitInfo::default();

        for proj in graph.toposort()? {
            let age = if let Some(ri) = rel_info.lookup_project(proj) {
                if proj.version.to_string() == ri.version {
                    ri.age + 1
                } else {
                    0
                }
            } else {
                0
            };

            info.projects.push(ReleasedProjectInfo {
                qnames: proj.qualified_names().clone(),
                version: proj.version.to_string(),
                age,
            });
        }

        // TODO: summary should say (e.g.) "Release cranko 0.1.0" if possible.
        let message = format!(
            "Release commit created with Cranko.

+++ cranko-release-info-v1
{}
+++
",
            toml::to_string(&info)?
        );

        // Turn the current index into a Tree.

        let tree_oid = {
            let mut index = self.repo.index()?;
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

        let commit_id = if let Some(prev_cid) = rel_info.commit {
            let prev_release_commit = self.repo.find_commit(prev_cid.0)?;
            commit(&[&prev_release_commit, &head_commit])?
        } else {
            commit(&[&head_commit])?
        };

        // Switch the working directory to be the checkout of our new merge
        // commit. By construction, nothing on the filesystem should actually
        // change.

        info!("switching HEAD to `{}`", local_ref_name);
        self.repo.set_head(&local_ref_name)?;
        self.repo.reset(
            self.repo.find_commit(commit_id)?.as_object(),
            git2::ResetType::Mixed,
            None,
        )?;

        // Phew, all done!

        Ok(())
    }

    /// Get information about a release from the HEAD commit.
    pub fn parse_release_info_from_head(&self) -> Result<ReleaseCommitInfo> {
        let head_ref = self.repo.head()?;
        let head_commit = head_ref.peel_to_commit()?;
        self.parse_release_info_from_commit(&head_commit)
    }

    /// Get information about a release from the HEAD commit.
    fn parse_release_info_from_commit(&self, commit: &git2::Commit) -> Result<ReleaseCommitInfo> {
        let msg = commit.message().ok_or_else(|| OldError::NotUnicodeError)?;

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
            } else if line.starts_with("+++ cranko-release-info-v1") {
                in_body = true;
            }
        }

        if in_body {
            println!("unterminated release info body; trying to proceed anyway");
        }

        if data.len() == 0 {
            return Err(OldError::InvalidCommitMessageFormat.into());
        }

        let srci: SerializedReleaseCommitInfo = toml::from_str(&data)?;

        Ok(ReleaseCommitInfo {
            commit: Some(CommitId(commit.id())),
            projects: srci.projects,
        })
    }

    /// Figure out which commits in the history affect each project since its
    /// last release.
    ///
    /// This gets a little tricky since not all projects in the repo are
    /// released in lockstep. For each individiual project, we need to analyze
    /// the history from HEAD to its most recent release commit. I worry about
    /// the efficiency of this so we trace all the histories at once to try to
    /// improve that.
    pub fn analyze_histories(&self, projects: &[Project]) -> Result<Vec<RepoHistory>> {
        // Here we (ab)use the fact that we know the project IDs are just a
        // simple usize sequence 0..n.
        let mut histories = vec![
            RepoHistory {
                commits: Vec::new(),
                release_commit: None,
            };
            projects.len()
        ];

        // First we dig through the history of the `release` branch to figure
        // out the most recent release for each project. In `release_commits`,
        // None indicates that the project has not yet been released. Here we
        // just naively scan the full project list every time -- unlikely that
        // it would be worthwhile to try something more clever?

        let latest_release_commit = self.try_get_release_commit()?;

        if let Some(mut commit) = latest_release_commit {
            let mut n_found = 0;

            loop {
                let rel_info = self.parse_release_info_from_commit(&commit)?;

                for (i, proj) in projects.iter().enumerate() {
                    if histories[i].release_commit.is_none()
                        && rel_info.lookup_if_released(proj).is_some()
                    {
                        histories[i].release_commit = Some(CommitId(commit.id()));
                        n_found += 1;
                    }
                }

                if n_found == projects.len() {
                    break; // ok, we got them all!
                }

                if commit.parent_count() == 1 {
                    // If a `release` commit has one parent, it is the first
                    // Cranko release commit in the project history, and all
                    // further parent commits are just regular code from
                    // `master` (because all other Cranko release commits merge
                    // the main branch into the release branch). Therefore any
                    // leftover projects must have no Cranko releases on record.
                    break;
                }

                commit = commit.parent(0)?;
            }
        }

        // Now that we have those, trace the history from HEAD to latest release
        // for each project, with some LRU caches to try to make things more
        // efficient. (I haven't done any testing to see how much the caching
        // helps, though ...)

        let mut commit_data = lru::LruCache::new(512);
        let mut trees = lru::LruCache::new(3);

        let mut dopts = git2::DiffOptions::new();
        dopts.include_typechange(true);

        // note that we don't "know" that proj_idx = project.ident
        for proj_idx in 0..projects.len() {
            let mut walk = self.repo.revwalk()?;
            walk.push_head()?;

            if let Some(release_commit_id) = histories[proj_idx].release_commit {
                walk.hide(release_commit_id.0)?;
            }

            // Walk through the history, finding relevant commits. The full
            // codepath loads up trees for each commit and its parents, computes
            // the diff, and compares that against the path-matchers for each
            // project to decide if a given commit affects a given project. The
            // intention is that the LRU caches will make it so that little
            // redundant work is performed.

            for maybe_oid in walk {
                let oid = maybe_oid?;

                // Hopefully this commit is already in the cache, but if not ...
                if !commit_data.contains(&oid) {
                    // Get the two relevant trees and compute their diff. We have to
                    // jump through some hoops to support the root commit (with no
                    // parents) but it's not really that bad. We also have to pop() the
                    // trees out of the LRU because get() holds a mutable reference to
                    // the cache, which prevents us from looking at two trees
                    // simultaneously.

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
                    // projects, are affected. Vec<bool> is a bit of a silly way to
                    // store the info, but hopefully good enough.

                    let mut hit_buf = vec![false; projects.len()];

                    for delta in diff.deltas() {
                        for file in &[delta.old_file(), delta.new_file()] {
                            if let Some(path_bytes) = file.path_bytes() {
                                let path = RepoPath::new(path_bytes);
                                for (idx, proj) in projects.iter().enumerate() {
                                    if proj.repo_paths.repo_path_matches(path) {
                                        hit_buf[idx] = true;
                                    }
                                }
                            }
                        }
                    }

                    // Save the information for posterity
                    commit_data.put(oid.clone(), hit_buf);
                }

                // OK, now the commit data is definitely in the cache.
                let hits = commit_data.get(&oid).unwrap();

                if hits[proj_idx] {
                    histories[proj_idx].commits.push(CommitId(oid));
                }
            }
        }

        Ok(histories)
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
        dirty_allowed: bool,
    ) -> Result<Option<RcProjectInfo>> {
        let mut saw_changelog = false;
        let changelog_matcher = proj.changelog.create_path_matcher(proj)?;

        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        opts.include_ignored(true);

        for entry in self.repo.statuses(Some(&mut opts))?.iter() {
            let path = RepoPath::new(entry.path_bytes());
            if !proj.repo_paths.repo_path_matches(path) {
                continue;
            }

            let status = entry.status();

            if changelog_matcher.repo_path_matches(path) {
                if status.is_conflicted() {
                    return Err(OldError::DirtyRepository(path.escaped()).into());
                } else if status.is_index_new()
                    || status.is_index_modified()
                    || status.is_wt_new()
                    || status.is_wt_modified()
                {
                    changes.add_path(path);
                    saw_changelog = true;
                } // TODO: handle/complain about some other statuses
            } else {
                if status.is_ignored() || status.is_wt_new() || status == git2::Status::CURRENT {
                } else if !dirty_allowed {
                    return Err(OldError::DirtyRepository(path.escaped()).into());
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
            .ok_or_else(|| OldError::NotUnicodeError)?;

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
            return Err(OldError::InvalidCommitMessageFormat.into());
        }

        let srci: SerializedRcCommitInfo = toml::from_str(&data)?;

        Ok(RcCommitInfo {
            commit: Some(CommitId(head_commit.id())),
            projects: srci.projects,
        })
    }

    /// Update the specified files in the working tree to reset them to what
    /// HEAD says they should be.
    pub fn hard_reset_changes(&self, changes: &ChangeList) -> Result<()> {
        // If no changes, do nothing. If we don't special-case this, the
        // checkout_head() will affect *all* files, i.e. perform a hard reset to
        // HEAD.
        if changes.paths.len() == 0 {
            return Ok(());
        }

        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force();

        // The key is that by specifying paths here, the checkout operation will
        // only affect those paths and not anything else.
        for path in &changes.paths[..] {
            let p: &RepoPath = path.as_ref();
            cb.path(p);
        }

        self.repo.checkout_head(Some(&mut cb))?;
        Ok(())
    }

    /// Get a tag name for a release of this project.
    pub fn get_tag_name(&self, proj: &Project, rel: &ReleasedProjectInfo) -> Result<String> {
        let mut tagname_args = HashMap::new();
        tagname_args.insert("project_slug", proj.user_facing_name.to_owned());
        tagname_args.insert("version", rel.version.clone());
        Ok(SimpleCurlyFormat
            .format(&self.release_tag_name_format, &tagname_args)
            .map_err(|e| Error::msg(e.to_string()))?
            .to_string())
    }

    /// Create a tag for a project release pointing to HEAD.
    pub fn tag_project_at_head(&self, proj: &Project, rel: &ReleasedProjectInfo) -> Result<()> {
        let head_ref = self.repo.head()?;
        let head_commit = head_ref.peel_to_commit()?;
        let sig = self.get_signature()?;
        let tagname = self.get_tag_name(proj, rel)?;

        self.repo
            .tag(&tagname, head_commit.as_object(), &sig, &tagname, false)?;

        info!(
            "created tag {} pointing at HEAD ({})",
            &tagname,
            head_commit.as_object().short_id()?.as_str().unwrap()
        );

        Ok(())
    }

    /// Find the earliest release of the specified project that contains
    /// the specified commit. If that commit has not yet been released,
    /// None is returned.
    pub fn find_earliest_release_containing(
        &self,
        proj: &Project,
        cid: &CommitId,
    ) -> Result<CommitAvailability> {
        let maybe_rpi = self.find_published_release_containing(proj, cid)?;

        if let Some(rpi) = maybe_rpi {
            let v = Version::parse_like(&proj.version, rpi.version)?;
            return Ok(CommitAvailability::ExistingRelease(v));
        }

        let head_ref = self.repo.head()?;
        let head_commit = head_ref.peel_to_commit()?;

        if self.repo.graph_descendant_of(head_commit.id(), cid.0)? {
            Ok(CommitAvailability::NewRelease)
        } else {
            Ok(CommitAvailability::NotAvailable)
        }
    }

    /// Find the earliest release of the specified project that contains
    /// the specified commit. If that commit has not yet been released,
    /// None is returned.
    fn find_published_release_containing(
        &self,
        proj: &Project,
        cid: &CommitId,
    ) -> Result<Option<ReleasedProjectInfo>> {
        let mut best_info = None;

        let mut commit = if let Some(c) = self.try_get_release_commit()? {
            c
        } else {
            // If no `release` branch, nothing's been released, so:
            return Ok(None);
        };

        loop {
            if !self.repo.graph_descendant_of(commit.id(), cid.0)? {
                // If this release commit is not a descendant of the desired
                // commit, we've gone too far back in the history -- quit.
                break;
            }

            let release = self.parse_release_info_from_commit(&commit)?;

            // Is the release of the project described in this commit older than
            // any other release that we've encountered? Probably! But we don't
            // want to make overly restrictive assumptions about commit
            // ordering.

            if let Some(cur_release) = release.lookup_if_released(proj) {
                let cur_version = proj.version.parse_like(&cur_release.version)?;

                if let Some((_, ref best_version)) = best_info {
                    if cur_version < *best_version {
                        best_info = Some((cur_release.clone(), cur_version));
                    }
                } else {
                    best_info = Some((cur_release.clone(), cur_version));
                }
            }

            if commit.parent_count() == 1 {
                // If a `release` commit has one parent, it is the first
                // Cranko release commit in the project history, so there's
                // nothing more to check.
                break;
            }

            commit = commit.parent(0)?;
        }

        Ok(best_info.map(|pair| pair.0))
    }
}

/// Information about the state of the projects in the repository corresponding
/// to a "release" commit where all of the projects have been assigned version
/// numbers, and the commit should have made it out into the wild only if all of
/// the CI tests passed.
#[derive(Clone, Debug, Default)]
pub struct ReleaseCommitInfo {
    /// The Git commit-ish that this object describes. May be None when there is
    /// no upstream `release` branch, in which case this struct will contain no
    /// genuine information.
    pub commit: Option<CommitId>,

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

    /// Find information about a project release if it occurred at this moment.
    ///
    /// This function is like `lookup_project()`, but also returns None if the
    /// "age" of any identified release is not zero.
    pub fn lookup_if_released(&self, proj: &Project) -> Option<&ReleasedProjectInfo> {
        self.lookup_project(proj).filter(|rel| rel.age == 0)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SerializedReleaseCommitInfo {
    pub projects: Vec<ReleasedProjectInfo>,
}

/// Serializable state information about a single project in a release commit.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Default)]
pub struct RcCommitInfo {
    /// The Git commit-ish that this object describes.
    pub commit: Option<CommitId>,

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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SerializedRcCommitInfo {
    pub projects: Vec<RcProjectInfo>,
}

/// Serializable state information about a single project with a proposed
/// release in an `rc` commit.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RcProjectInfo {
    /// The qualified names of this project, equivalent to the same-named
    /// property of the Project struct.
    pub qnames: Vec<String>,

    /// The kind of version bump requested by the user.
    pub bump_spec: String,
}

/// Describes the release availability of a particular commit in a project's
/// history. Note that for the same commit, this information might vary
/// depending on which project we're talking about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitAvailability {
    /// The commit has already been released, and the earliest release
    /// containing it has the given version.
    ExistingRelease(Version),

    /// The commit has not been released but is an ancestor of HEAD, so it would
    /// be available if a new release of the target project were to be created.
    /// We need to pay attention to this case to allow people to stage and
    /// release multiple projects in one batch.
    NewRelease,

    /// None of the above.
    NotAvailable,
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

#[derive(Clone, Debug, PartialEq)]
pub struct RepoHistory {
    commits: Vec<CommitId>,
    release_commit: Option<CommitId>,
}

impl RepoHistory {
    /// Get the Cranko release commit that this chunk of history
    /// extends to. If None, there is no such commit, and the
    /// history extends all the way to the start of the project
    /// history.
    pub fn release_commit(&self) -> Option<CommitId> {
        self.release_commit
    }

    /// Get the release information corresponding to this item's release commit.
    /// If this history item was computed for certain project, this info is
    /// guaranteed to contain an age=0 release for that project (if it is not
    /// None).
    pub fn release_info(&self, repo: &Repository) -> Result<Option<ReleaseCommitInfo>> {
        if let Some(cid) = self.release_commit() {
            let commit = repo.repo.find_commit(cid.0)?;
            Ok(Some(repo.parse_release_info_from_commit(&commit)?))
        } else {
            Ok(None)
        }
    }

    /// Get the number of commits in this chunk of history.
    pub fn n_commits(&self) -> usize {
        self.commits.len()
    }

    /// Get the commit IDs in this chunk of history.
    pub fn commits(&self) -> impl IntoIterator<Item = &CommitId> {
        &self.commits[..]
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

/// A reference to a commit in the repository. We have some special machinery to
/// allow people to create commits that reference themselves.
pub enum CommitRef {
    /// A reference to a specific commit ID
    Id(CommitId),

    /// A reference to the commit that introduced this reference into the
    /// repository contents. `salt` is a random string allowing different
    /// this-commit references to be distinguished and to ease identification of
    /// the relevant commit through "blame" tracing of the repository history.
    ThisCommit { salt: String },
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
                return Err(OldError::OutsideOfRepository(format!(
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

    pub fn push<C: AsRef<[u8]>>(&mut self, component: C) {
        let n = self.0.len();

        if n > 0 && self.0[n - 1] != b'/' {
            self.0.push(b'/');
        }

        self.0.extend(component.as_ref());
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
