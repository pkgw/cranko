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

use crate::{
    app::AppSession,
    errors::{Error, Result},
    project::Project,
    repository::{ChangeList, CommitId, RcProjectInfo, RepoPath, RepoPathBuf, Repository},
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

    /// Test whether a path in the working directory, relative to the working
    /// directory root, corresponds to a changelog file. Used in "cranko
    /// confirm" to detect staged projects and make sure that there are no
    /// functional changes.
    fn is_changelog_path_for(&self, proj: &Project, path: &RepoPath) -> bool;

    /// Scan the changelog(s) in the project's working directory to extract
    /// metadata about a proposed release of the project. The idea is that we
    /// (ab)use the changelog to allow the user to codify the release metadata
    /// used in the RcProjectInfo type.
    fn scan_rc_info(&self, proj: &Project, repo: &Repository) -> Result<RcProjectInfo>;

    /// Rewrite the changelog file(s) in the project's working directory, which
    /// are in the "rc" format that includes release candidate metadata, to
    /// instead include the final release information.
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
    Box::new(MarkdownChangelog::default())
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
}

impl Changelog for MarkdownChangelog {
    fn draft_release_update(
        &self,
        proj: &Project,
        sess: &AppSession,
        changes: &[CommitId],
        prev_release_commit: Option<CommitId>,
    ) -> Result<()> {
        // Populate the previous changelog from the most recent `release`
        // commit, if available. This gives the opportunity to refer to the
        // historical changelog entries for stylistic reference and potentially
        // fix mistakes.

        let changelog_repopath = self.changelog_repopath(proj);

        let prev_log = if let Some(prc) = prev_release_commit {
            sess.repo.get_file_at_commit(&prc, &changelog_repopath)?
        } else {
            Vec::new()
        };

        // Now populate the augmented log.

        let changelog_path = self.changelog_path(proj, &sess.repo);

        let new_af = atomicwrites::AtomicFile::new(
            &changelog_path,
            atomicwrites::OverwriteBehavior::AllowOverwrite,
        );

        let r = new_af.write(|new_f| {
            // Header

            let mut headfoot_args = HashMap::new();
            headfoot_args.insert("bump_spec", "micro bump");
            let header = SimpleCurlyFormat.format(&self.stage_header_format, &headfoot_args)?;
            writeln!(new_f, "{}", header)?;

            // Commit summaries! Note: if we're staging muliple projects and the
            // same commit affects many of them, we'll reload the same commit many
            // times when generating changelogs.

            const WRAP_WIDTH: usize = 78;

            for cid in changes {
                let message = sess.repo.get_commit_summary(*cid)?;
                let mut prefix = "- ";

                for line in textwrap::wrap_iter(&message, WRAP_WIDTH) {
                    writeln!(new_f, "{}{}", prefix, line)?;
                    prefix = "  ";
                }
            }

            // Footer

            let footer = SimpleCurlyFormat.format(&self.footer_format, &headfoot_args)?;
            writeln!(new_f, "{}", footer)?;

            // Write back all of the previous contents, and we're done.
            new_f.write_all(&prev_log[..])?;

            Ok(())
        });

        match r {
            Err(atomicwrites::Error::Internal(e)) => Err(e.into()),
            Err(atomicwrites::Error::User(e)) => Err(e),
            Ok(()) => Ok(()),
        }
    }

    fn is_changelog_path_for(&self, proj: &Project, path: &RepoPath) -> bool {
        let pfx = proj.prefix();

        if !path.starts_with(pfx) {
            return false;
        }

        if path.len() != pfx.len() + self.basename.len() {
            return false;
        }

        return path.ends_with(self.basename.as_bytes());
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

            if line.starts_with("# rc:") {
                let spec = line[5..].trim();
                bump_spec = Some(spec.to_owned());
                break;
            }

            return Err(Error::InvalidChangelogFormat(
                changelog_path.display().to_string(),
            ));
        }

        let bump_spec = bump_spec
            .ok_or_else(|| Error::InvalidChangelogFormat(changelog_path.display().to_string()))?;
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
                            return Err(Error::InvalidChangelogFormat(
                                changelog_path.display().to_string(),
                            ));
                        }

                        state = State::BlanksAfterHeader;
                        let header =
                            SimpleCurlyFormat.format(&self.release_header_format, &header_args)?;
                        writeln!(new_f, "{}", header)?;
                    }

                    State::BlanksAfterHeader => {
                        if !line.trim().is_empty() {
                            state = State::AfterHeader;
                            writeln!(new_f, "{}", line)?;
                        }
                    }

                    State::AfterHeader => {
                        writeln!(new_f, "{}", line)?;
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
        let data = repo.get_file_at_commit(cid, &changelog_path)?;
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
