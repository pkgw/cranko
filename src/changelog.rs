// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Dealing with changelogs.
//!
//! This whole subject matter might not seem integral to the operation of
//! Cranko, but it turns out to be very relevant, since so much of Cranko's core
//! has to do with looking at the repository history since the most recent
//! release(s). That's exactly the information contained in a release changelog.

use chrono::{offset::Local, Datelike};
use dynfmt::{Format, SimpleCurlyFormat};
use std::{
    collections::HashMap,
    fs::File,
    io::{prelude::*, BufReader, Cursor},
    path::PathBuf,
};
use thiserror::Error as ThisError;

use crate::{
    app::AppSession,
    errors::{Error, Result},
    project::Project,
    repository::{ChangeList, CommitId, PathMatcher, RcProjectInfo, RepoPathBuf, Repository},
    write_crlf,
};

/// A type that defines how the changelog for a given project is managed.
pub trait Changelog: std::fmt::Debug {
    /// Rewrite the changelog file(s) with stub contents derived from the
    /// repository history, prepended to whatever contents existed at the
    /// previous release commit.
    fn draft_release_update(
        &self,
        proj: &Project,
        sess: &AppSession,
        changes: &[CommitId],
        prev_release_commit: Option<CommitId>,
    ) -> Result<()>;

    /// Replace the changelog file(s) in the project's working directory with
    /// the contents from the most recent release of the project.
    fn replace_changelog(
        &self,
        proj: &Project,
        sess: &AppSession,
        changes: &mut ChangeList,
        prev_release_commit: CommitId,
    ) -> Result<()>;

    /// Create a matcher that matches one or more paths in the project's
    /// directory corresponding to its changelog(s). Operations like `cranko
    /// stage` and `cranko confirm` care about working directory dirtiness, but
    /// in our model modified changelogs are OK.
    fn create_path_matcher(&self, proj: &Project) -> Result<PathMatcher>;

    /// Scan the changelog(s) in the project's working directory to extract
    /// metadata about a proposed release of the project. The idea is that we
    /// (ab)use the changelog to allow the user to codify the release metadata
    /// used in the RcProjectInfo type.
    fn scan_rc_info(&self, proj: &Project, repo: &Repository) -> Result<RcProjectInfo>;

    /// Rewrite the changelog file(s) in the project's working directory, which
    /// are in the "rc" format that includes release candidate metadata, to
    /// instead include the final release information. The changelog contents
    /// will already include earlier entries.
    fn finalize_changelog(
        &self,
        proj: &Project,
        repo: &Repository,
        changes: &mut ChangeList,
    ) -> Result<()>;

    /// Read most recent changelog text *as of the specified commit*.
    ///
    /// For now, this text is presumed to be formatted in CommonMark format.
    ///
    /// Note that this operation ignores the working tree in an effort to provide
    /// more reliability.
    fn scan_changelog(&self, proj: &Project, repo: &Repository, cid: &CommitId) -> Result<String>;
}

/// Create a new default Changelog implementation.
///
/// This uses the Markdown format.
pub fn default() -> Box<dyn Changelog> {
    Box::<MarkdownChangelog>::default()
}

/// An error returned when a changelog file does not obey the special structure
/// expected by Cranko's processing routines. The inner value is the path to the
/// offending changelog (not a RepoPathBuf since it may not have yet been added
/// to the repo).
#[derive(Debug, ThisError)]
pub struct InvalidChangelogFormatError(pub PathBuf);

impl std::fmt::Display for InvalidChangelogFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "changelog file `{}` does not obey the expected formatting",
            self.0.display()
        )
    }
}

/// Settings for Markdown-formatted changelogs.
#[derive(Debug)]
pub struct MarkdownChangelog {
    basename: String,
    release_header_format: String,
    stage_header_format: String,
    footer_format: String,
}

impl Default for MarkdownChangelog {
    fn default() -> Self {
        MarkdownChangelog {
            basename: "CHANGELOG.md".to_owned(),
            release_header_format: "# {project_slug} {version} ({yyyy_mm_dd})\n".to_owned(),
            stage_header_format: "# rc: {bump_spec}\n".to_owned(),
            footer_format: "".to_owned(),
        }
    }
}

impl MarkdownChangelog {
    fn changelog_repopath(&self, proj: &Project) -> RepoPathBuf {
        let mut pfx = proj.prefix().to_owned();
        pfx.push(&self.basename);
        pfx
    }

    fn changelog_path(&self, proj: &Project, repo: &Repository) -> PathBuf {
        repo.resolve_workdir(&self.changelog_repopath(proj))
    }

    /// Generic implementation for draft_release_update and replace_changelog.
    fn replace_changelog_impl(
        &self,
        proj: &Project,
        sess: &AppSession,
        prev_release_commit: Option<CommitId>,
        in_changes: Option<&[CommitId]>,
        out_changes: Option<&mut ChangeList>,
    ) -> Result<()> {
        // Get the previous changelog from the most recent `release`
        // commit.

        let changelog_repopath = self.changelog_repopath(proj);

        let prev_log: Vec<u8> = prev_release_commit
            .map(|prc| sess.repo.get_file_at_commit(&prc, &changelog_repopath))
            .transpose()?
            .flatten()
            .unwrap_or_default();

        // Start working on rewriting the existing file.

        let changelog_path = self.changelog_path(proj, &sess.repo);

        let new_af = atomicwrites::AtomicFile::new(
            changelog_path,
            atomicwrites::OverwriteBehavior::AllowOverwrite,
        );

        let r = new_af.write(|new_f| {
            if let Some(commits) = in_changes {
                // We're drafting a release update -- add a new section.

                let mut headfoot_args = HashMap::new();
                headfoot_args.insert("bump_spec", "micro bump");
                let header = SimpleCurlyFormat
                    .format(&self.stage_header_format, &headfoot_args)
                    .map_err(|e| Error::msg(e.to_string()))?;
                write_crlf!(new_f, "{}", header)?;

                // Commit summaries! Note: if we're staging muliple projects and the
                // same commit affects many of them, we'll reload the same commit many
                // times when generating changelogs.

                const WRAP_WIDTH: usize = 78;

                for cid in commits {
                    let message = sess.repo.get_commit_summary(*cid)?;
                    let mut prefix = "- ";

                    for line in textwrap::wrap(&message, WRAP_WIDTH) {
                        write_crlf!(new_f, "{}{}", prefix, line)?;
                        prefix = "  ";
                    }
                }

                // Footer

                let footer = SimpleCurlyFormat
                    .format(&self.footer_format, &headfoot_args)
                    .map_err(|e| Error::msg(e.to_string()))?;
                write_crlf!(new_f, "{}", footer)?;
            }

            // Write back all of the previous contents, and we're done.
            new_f.write_all(&prev_log[..])?;

            Ok(())
        });

        if let Some(chlist) = out_changes {
            chlist.add_path(&self.changelog_repopath(proj));
        }

        match r {
            Err(atomicwrites::Error::Internal(e)) => Err(e.into()),
            Err(atomicwrites::Error::User(e)) => Err(e),
            Ok(()) => Ok(()),
        }
    }
}

impl Changelog for MarkdownChangelog {
    fn draft_release_update(
        &self,
        proj: &Project,
        sess: &AppSession,
        changes: &[CommitId],
        prev_release_commit: Option<CommitId>,
    ) -> Result<()> {
        self.replace_changelog_impl(proj, sess, prev_release_commit, Some(changes), None)
    }

    fn replace_changelog(
        &self,
        proj: &Project,
        sess: &AppSession,
        changes: &mut ChangeList,
        prev_release_commit: CommitId,
    ) -> Result<()> {
        self.replace_changelog_impl(proj, sess, Some(prev_release_commit), None, Some(changes))
    }

    fn create_path_matcher(&self, proj: &Project) -> Result<PathMatcher> {
        Ok(PathMatcher::new_include(self.changelog_repopath(proj)))
    }

    fn scan_rc_info(&self, proj: &Project, repo: &Repository) -> Result<RcProjectInfo> {
        let changelog_path = self.changelog_path(proj, repo);
        let f = File::open(&changelog_path)?;
        let reader = BufReader::new(f);
        let mut bump_spec = None;

        // We allow all-whitespace lines before the rc: header, but that's it.
        for maybe_line in reader.lines() {
            let line = maybe_line?;
            if line.trim().is_empty() {
                continue;
            }

            if let Some(spec_text) = line.strip_prefix("# rc:") {
                let spec = spec_text.trim();
                bump_spec = Some(spec.to_owned());
                break;
            }

            return Err(InvalidChangelogFormatError(changelog_path).into());
        }

        let bump_spec = bump_spec.ok_or(InvalidChangelogFormatError(changelog_path))?;
        let _check_scheme = proj.version.parse_bump_scheme(&bump_spec)?;

        Ok(RcProjectInfo {
            qnames: proj.qualified_names().clone(),
            bump_spec,
        })
    }

    fn finalize_changelog(
        &self,
        proj: &Project,
        repo: &Repository,
        changes: &mut ChangeList,
    ) -> Result<()> {
        // Prepare the substitution template
        let mut header_args = HashMap::new();
        header_args.insert("project_slug", proj.user_facing_name.to_owned());
        header_args.insert("version", proj.version.to_string());
        let now = Local::now();
        header_args.insert(
            "yyyy_mm_dd",
            format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day()),
        );

        let changelog_path = self.changelog_path(proj, repo);
        let cur_f = File::open(&changelog_path)?;
        let cur_reader = BufReader::new(cur_f);

        let new_af = atomicwrites::AtomicFile::new(
            &changelog_path,
            atomicwrites::OverwriteBehavior::AllowOverwrite,
        );
        let r = new_af.write(|new_f| {
            // Pipe the current changelog into the new one, replacing the `rc`
            // header with the final one.

            #[allow(clippy::enum_variant_names)]
            enum State {
                BeforeHeader,
                BlanksAfterHeader,
                AfterHeader,
            }
            let mut state = State::BeforeHeader;

            for maybe_line in cur_reader.lines() {
                let line = maybe_line?;

                match state {
                    State::BeforeHeader => {
                        if line.trim().is_empty() {
                            continue;
                        }

                        if !line.starts_with("# rc:") {
                            return Err(InvalidChangelogFormatError(changelog_path).into());
                        }

                        state = State::BlanksAfterHeader;
                        let header = SimpleCurlyFormat
                            .format(&self.release_header_format, &header_args)
                            .map_err(|e| Error::msg(e.to_string()))?;
                        write_crlf!(new_f, "{}", header)?;
                    }

                    State::BlanksAfterHeader => {
                        if !line.trim().is_empty() {
                            state = State::AfterHeader;
                            write_crlf!(new_f, "{}", line)?;
                        }
                    }

                    State::AfterHeader => {
                        write_crlf!(new_f, "{}", line)?;
                    }
                }
            }

            Ok(())
        });

        changes.add_path(&self.changelog_repopath(proj));

        match r {
            Err(atomicwrites::Error::Internal(e)) => Err(e.into()),
            Err(atomicwrites::Error::User(e)) => Err(e),
            Ok(()) => Ok(()),
        }
    }

    fn scan_changelog(&self, proj: &Project, repo: &Repository, cid: &CommitId) -> Result<String> {
        let changelog_path = self.changelog_repopath(proj);
        let data = match repo.get_file_at_commit(cid, &changelog_path)? {
            Some(d) => d,
            None => return Ok(String::new()),
        };
        let reader = Cursor::new(data);

        enum State {
            BeforeHeader,
            InChangelog,
        }
        let mut state = State::BeforeHeader;
        let mut changelog = String::new();

        // In a slight tweak from other methods here, we ignore everything
        // before a "# " header.
        for maybe_line in reader.lines() {
            let line = maybe_line?;

            match state {
                State::BeforeHeader => {
                    if line.starts_with("# ") {
                        changelog.push_str(&line);
                        changelog.push('\n');
                        state = State::InChangelog;
                    }
                }

                State::InChangelog => {
                    if line.starts_with("# ") {
                        break;
                    } else {
                        changelog.push_str(&line);
                        changelog.push('\n');
                    }
                }
            }
        }

        Ok(changelog)
    }
}
