// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Version numbers.

use chrono::{offset::Local, Datelike};

use crate::{
    errors::{Error, Result},
    repository::ReleasedProjectInfo,
};

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

impl Version {
    /// Given a template version, parse a "bump scheme" from a textual
    /// description.
    ///
    /// Not all bump schemes are compatible with all versioning styles, which is
    /// why this operation depends on the version template and is fallible.
    pub fn parse_bump_scheme(&self, text: &str) -> Result<VersionBumpScheme> {
        match text {
            "micro bump" => Ok(VersionBumpScheme::MicroBump),
            "minor bump" => Ok(VersionBumpScheme::MinorBump),
            "major bump" => Ok(VersionBumpScheme::MajorBump),
            _ => Err(Error::UnsupportedBumpScheme(text.to_owned(), self.clone())),
        }
    }
}

/// A scheme for assigning a new version number to a project.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionBumpScheme {
    /// Assigns a development-mode version (likely 0.0.0) with a YYYYMMDD date code included.
    DevDatecode,

    /// Increment the third-most-significant version number, resetting any
    /// less-significant entries.
    MicroBump,

    /// Increment the second-most-significant version number, resetting any
    /// less-significant entries.
    MinorBump,

    /// Increment the most-significant version number, resetting any
    /// less-significant entries.
    MajorBump,
}

impl VersionBumpScheme {
    /// Generate a new version by applying a versioning scheme to a template
    /// version, in a specified release mode, potentially building off of
    /// information about the most recent prior release.
    pub fn apply(
        &self,
        template: &Version,
        latest_release: Option<&ReleasedProjectInfo>,
    ) -> Result<Version> {
        // This function inherently has to matrix over versioning schemes and
        // versioning systems, so it gets a little hairy.
        return match self {
            VersionBumpScheme::DevDatecode => apply_dev_datecode(template, latest_release),
            VersionBumpScheme::MicroBump => apply_micro_bump(template, latest_release),
            VersionBumpScheme::MinorBump => apply_minor_bump(template, latest_release),
            VersionBumpScheme::MajorBump => apply_major_bump(template, latest_release),
        };

        fn apply_dev_datecode(
            template: &Version,
            latest_release: Option<&ReleasedProjectInfo>,
        ) -> Result<Version> {
            let local = Local::now();
            let code = format!("{:04}{:02}{:02}", local.year(), local.month(), local.day());

            match template {
                Version::Semver(_) => {
                    let mut v = if let Some(rpi) = latest_release {
                        semver::Version::parse(&rpi.version)?
                    } else {
                        semver::Version::new(0, 0, 0)
                    };

                    v.build.push(semver::Identifier::AlphaNumeric(code));
                    Ok(Version::Semver(v))
                }
            }
        }

        fn apply_micro_bump(
            template: &Version,
            latest_release: Option<&ReleasedProjectInfo>,
        ) -> Result<Version> {
            match template {
                Version::Semver(_) => {
                    let mut v = if let Some(rpi) = latest_release {
                        semver::Version::parse(&rpi.version)?
                    } else {
                        semver::Version::new(0, 0, 0)
                    };

                    v.pre.clear();
                    v.build.clear();
                    v.patch += 1;

                    Ok(Version::Semver(v))
                }
            }
        }

        fn apply_minor_bump(
            template: &Version,
            latest_release: Option<&ReleasedProjectInfo>,
        ) -> Result<Version> {
            match template {
                Version::Semver(_) => {
                    let mut v = if let Some(rpi) = latest_release {
                        semver::Version::parse(&rpi.version)?
                    } else {
                        semver::Version::new(0, 0, 0)
                    };

                    v.pre.clear();
                    v.build.clear();
                    v.patch = 0;
                    v.minor += 1;

                    Ok(Version::Semver(v))
                }
            }
        }

        fn apply_major_bump(
            template: &Version,
            latest_release: Option<&ReleasedProjectInfo>,
        ) -> Result<Version> {
            match template {
                Version::Semver(_) => {
                    let mut v = if let Some(rpi) = latest_release {
                        semver::Version::parse(&rpi.version)?
                    } else {
                        semver::Version::new(0, 0, 0)
                    };

                    v.pre.clear();
                    v.build.clear();
                    v.patch = 0;
                    v.minor = 0;
                    v.major += 1;

                    Ok(Version::Semver(v))
                }
            }
        }
    }
}
