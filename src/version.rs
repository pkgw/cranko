// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Version numbers.

use chrono::{offset::Local, Datelike};

use crate::{errors::Result, repository::ReleasedProjectInfo};

/// A version number associated with a project.
///
/// This is an enumeration because different kinds of projects may subscribe to
/// different kinds of versioning schemes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Version {
    /// A version compatible with the semantic versioning specification.
    Semver(semver::Version),
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Version::Semver(ref v) => write!(f, "{}", v),
        }
    }
}

/// A scheme for assigning a version number to a project.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersioningScheme {
    /// Assigns a development-mode version (likely 0.0.0) with a YYYYMMDD date code included.
    DevDatecode,
}

impl VersioningScheme {
    /// Generate a new version by applying a versioning scheme to a template
    /// version, in a specified release mode, potentially building off of
    /// information about the most recent prior release.
    pub fn apply(
        &self,
        template: &Version,
        mode: ReleaseMode,
        latest_release: Option<&ReleasedProjectInfo>,
    ) -> Result<Version> {
        // This function inherently has to matrix over versioning schemes and
        // versioning systems, so it gets a little hairy.
        return match self {
            VersioningScheme::DevDatecode => apply_dev_datecode(template, mode, latest_release),
        };

        fn apply_dev_datecode(
            template: &Version,
            _mode: ReleaseMode,
            _latest_release: Option<&ReleasedProjectInfo>,
        ) -> Result<Version> {
            let local = Local::now();
            let code = format!("{:04}{:02}{:02}", local.year(), local.month(), local.day());

            match template {
                Version::Semver(_) => {
                    let mut v = semver::Version::new(0, 0, 0);
                    v.build.push(semver::Identifier::AlphaNumeric(code));
                    Ok(Version::Semver(v))
                }
            }
        }
    }
}

/// A release "mode" in which version numbers may be assigned.
///
/// Depending on this mode, different versioning schemes may be active. E.g.,
/// continuous deployment vs. an explicit request to make a primary release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseMode {
    /// An automated, continuous-deployment style of release.
    Development,

    /// A user-requested "primary" release that will be officially published.
    Primary,
}
