// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Dealing with changelogs.
//!
//! This whole subject matter might not seem integral to the operation of
//! Cranko, but it turns out to be very relevant, since so much of Cranko's core
//! has to do with looking at the repository history since the most recent
//! release(s). That's exactly the information contained in a release changelog.

//use chrono::{offset::Local, Datelike};
use dynfmt::{Format, SimpleCurlyFormat};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
};

use crate::{app::AppSession, errors::Result, project::Project, repository::CommitId};

/// How to format the changelog for a given project.
#[derive(Debug)]
pub enum ChangelogFormat {
    /// A standard Markdown-formatted changelog.
    Markdown(MarkdownFormat),
}

impl Default for ChangelogFormat {
    fn default() -> Self {
        ChangelogFormat::Markdown(MarkdownFormat::default())
    }
}

impl ChangelogFormat {
    /// Rewrite the changelog file with stub contents derived from the
    /// repository history.
    pub fn draft_release_update(
        &self,
        proj: &Project,
        sess: &AppSession,
        changes: &[CommitId],
    ) -> Result<()> {
        match self {
            ChangelogFormat::Markdown(f) => f.draft_release_update(proj, sess, changes),
        }
    }
}

/// Settings for Markdown-formatted changelogs.
#[derive(Debug)]
pub struct MarkdownFormat {
    basename: String,
    release_header_format: String,
    stage_header_format: String,
    footer_format: String,
}

impl Default for MarkdownFormat {
    fn default() -> Self {
        MarkdownFormat {
            basename: "CHANGELOG.md".to_owned(),
            release_header_format: "# Version {version} ({yyyy_mm_dd})\n\n".to_owned(),
            stage_header_format: "# rc: {bump_spec}\n\n".to_owned(),
            footer_format: "\n".to_owned(),
        }
    }
}

impl MarkdownFormat {
    fn changelog_path(&self, proj: &Project, repo: &Repository) -> PathBuf {
        let mut changelog_path = repo.resolve_workdir(proj.prefix());
        changelog_path.push(&self.basename);
        changelog_path
    }

    fn draft_release_update(
        &self,
        proj: &Project,
        sess: &AppSession,
        changes: &[CommitId],
    ) -> Result<()> {
        let changelog_path = self.changelog_path(proj, &sess.repo);

        let prev_log = {
            match File::open(&changelog_path) {
                Ok(mut f) => {
                    let mut data = Vec::new();
                    f.read_to_end(&mut data)?;
                    data
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        Vec::new() // no existing changelog? no problem!
                    } else {
                        return Err(e.into());
                    }
                }
            }
        };

        // TODO: write out as a tempfile, atomic rename, etc. (Though this is
        // less critical since the changelog is tracked in version control.)

        let mut f = File::create(changelog_path)?;

        // Header

        let mut headfoot_args = HashMap::new();
        headfoot_args.insert("bump_spec", "micro bump");
        let header = SimpleCurlyFormat.format(&self.stage_header_format, &headfoot_args)?;
        write!(f, "{}", header)?;

        // Commit summaries! Note: if we're staging muliple projects and the
        // same commit affects many of them, we'll reload the same commit many
        // times when generating changelogs.

        const WRAP_WIDTH: usize = 78;

        for cid in changes {
            let message = sess.repo.get_commit_summary(*cid)?;
            let mut prefix = "- ";

            for line in textwrap::wrap_iter(&message, WRAP_WIDTH) {
                write!(f, "{}{}\n", prefix, line)?;
                prefix = "  ";
            }
        }

        // Footer

        let footer = SimpleCurlyFormat.format(&self.footer_format, &headfoot_args)?;
        write!(f, "{}", footer)?;

        // Write back all of the previous contents, and we're done.
        f.write_all(&prev_log[..])?;
        Ok(())
    }
}
