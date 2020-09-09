// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Version numbers.

use chrono::{offset::Local, Datelike};

use crate::errors::{OldError, Result};

/// A version number associated with a project.
///
/// This is an enumeration because different kinds of projects may subscribe to
/// different kinds of versioning schemes.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd)]
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
    /// Given a template version, parse another version
    pub fn parse_like<T: AsRef<str>>(&self, text: T) -> Result<Version> {
        Ok(match self {
            Version::Semver(_) => Version::Semver(semver::Version::parse(text.as_ref())?),
        })
    }

    /// Given a template version, compute its "zero"
    pub fn zero_like(&self) -> Version {
        match self {
            Version::Semver(_) => Version::Semver(semver::Version::new(0, 0, 0)),
        }
    }

    /// Given a template version, parse a "bump scheme" from a textual
    /// description.
    ///
    /// Not all bump schemes are compatible with all versioning styles, which is
    /// why this operation depends on the version template and is fallible.
    pub fn parse_bump_scheme(&self, text: &str) -> Result<VersionBumpScheme> {
        if text.starts_with("force ") {
            return Ok(VersionBumpScheme::Force(text[6..].to_owned()));
        }

        match text {
            "micro bump" => Ok(VersionBumpScheme::MicroBump),
            "minor bump" => Ok(VersionBumpScheme::MinorBump),
            "major bump" => Ok(VersionBumpScheme::MajorBump),
            "dev-datecode" => Ok(VersionBumpScheme::DevDatecode),
            _ => Err(OldError::UnsupportedBumpScheme(text.to_owned(), self.clone()).into()),
        }
    }
}

/// A scheme for assigning a new version number to a project.
#[derive(Clone, Debug, Eq, PartialEq)]
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

    /// Force the version to the specified value.
    Force(String),
}

impl VersionBumpScheme {
    /// Apply this bump to a version.
    pub fn apply(&self, version: &mut Version) -> Result<()> {
        // This function inherently has to matrix over versioning schemes and
        // versioning systems, so it gets a little hairy.
        return match self {
            VersionBumpScheme::DevDatecode => apply_dev_datecode(version),
            VersionBumpScheme::MicroBump => apply_micro_bump(version),
            VersionBumpScheme::MinorBump => apply_minor_bump(version),
            VersionBumpScheme::MajorBump => apply_major_bump(version),
            VersionBumpScheme::Force(ref t) => apply_force(version, t),
        };

        fn apply_dev_datecode(version: &mut Version) -> Result<()> {
            let local = Local::now();
            let code = format!("{:04}{:02}{:02}", local.year(), local.month(), local.day());

            match version {
                Version::Semver(v) => {
                    v.build.push(semver::Identifier::AlphaNumeric(code));
                }
            }

            Ok(())
        }

        fn apply_micro_bump(version: &mut Version) -> Result<()> {
            match version {
                Version::Semver(v) => {
                    v.pre.clear();
                    v.build.clear();
                    v.patch += 1;
                }
            }

            Ok(())
        }

        fn apply_minor_bump(version: &mut Version) -> Result<()> {
            match version {
                Version::Semver(v) => {
                    v.pre.clear();
                    v.build.clear();
                    v.patch = 0;
                    v.minor += 1;
                }
            }

            Ok(())
        }

        fn apply_major_bump(version: &mut Version) -> Result<()> {
            match version {
                Version::Semver(v) => {
                    v.pre.clear();
                    v.build.clear();
                    v.patch = 0;
                    v.minor = 0;
                    v.major += 1;
                }
            }

            Ok(())
        }

        fn apply_force(version: &mut Version, text: &str) -> Result<()> {
            *version = version.parse_like(text)?;
            Ok(())
        }
    }
}
