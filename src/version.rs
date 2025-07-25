// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Version numbers.

use anyhow::bail;
use chrono::{offset::Local, Datelike};
use std::fmt::{Display, Formatter};
use thiserror::Error as ThisError;

use crate::errors::Result;

pub use dotnet::DotNetVersion;
pub use pep440::Pep440Version;

/// A version number associated with a project.
///
/// This is an enumeration because different kinds of projects may subscribe to
/// different kinds of versioning schemes.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd)]
pub enum Version {
    /// A version compatible with the semantic versioning specification.
    Semver(semver::Version),

    // A version compatible with the Python PEP-440 specification.
    Pep440(Pep440Version),

    // A version compatible with the .NET System.Version type.
    DotNet(DotNetVersion),
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter) -> std::result::Result<(), std::fmt::Error> {
        match self {
            Version::Semver(ref v) => write!(f, "{v}"),
            Version::Pep440(ref v) => write!(f, "{v}"),
            Version::DotNet(ref v) => write!(f, "{v}"),
        }
    }
}

impl Version {
    /// Given a template version, parse another version
    pub fn parse_like<T: AsRef<str>>(&self, text: T) -> Result<Version> {
        Ok(match self {
            Version::Semver(_) => Version::Semver(semver::Version::parse(text.as_ref())?),
            Version::Pep440(_) => Version::Pep440(text.as_ref().parse()?),
            Version::DotNet(_) => Version::DotNet(text.as_ref().parse()?),
        })
    }

    /// Given a template version, compute its "zero"
    pub fn zero_like(&self) -> Version {
        match self {
            Version::Semver(_) => Version::Semver(semver::Version::new(0, 0, 0)),
            Version::Pep440(_) => Version::Pep440(Pep440Version::default()),
            Version::DotNet(_) => Version::DotNet(DotNetVersion::default()),
        }
    }

    /// Mutate this version to be Cranko's default "development mode" value.
    pub fn set_to_dev_value(&mut self) {
        match self {
            Version::Semver(v) => {
                v.major = 0;
                v.minor = 0;
                v.patch = 0;
                v.pre = semver::Prerelease::new("dev.0").unwrap();
                v.build = semver::BuildMetadata::EMPTY;
            }

            Version::Pep440(v) => {
                v.epoch = 0;
                v.segments.clear();
                v.segments.push(0);
                v.pre_release = None;
                v.post_release = None;
                v.dev_release = Some(0);
                v.local_identifier = None;
            }

            Version::DotNet(v) => {
                // Quasi-hack for WWT
                v.minor = 99;
                v.build = 0;
                v.revision = 0;
            }
        }
    }

    /// Given a template version, parse a "bump scheme" from a textual
    /// description.
    ///
    /// Not all bump schemes are compatible with all versioning styles, which is
    /// why this operation depends on the version template and is fallible.
    #[allow(clippy::result_large_err)]
    pub fn parse_bump_scheme(
        &self,
        text: &str,
    ) -> std::result::Result<VersionBumpScheme, UnsupportedBumpSchemeError> {
        if let Some(force_text) = text.strip_prefix("force ") {
            return Ok(VersionBumpScheme::Force(force_text.to_owned()));
        }

        match text {
            "micro bump" => Ok(VersionBumpScheme::MicroBump),
            "minor bump" => Ok(VersionBumpScheme::MinorBump),
            "major bump" => Ok(VersionBumpScheme::MajorBump),
            "dev-datecode" => Ok(VersionBumpScheme::DevDatecode),
            _ => Err(UnsupportedBumpSchemeError(text.to_owned(), self.clone())),
        }
    }

    pub fn as_pep440_tuple_literal(&self) -> Result<String> {
        if let Version::Pep440(v) = self {
            v.as_tuple_literal()
        } else {
            bail!("version {} cannot be rendered as a PEP440 literal since it is not a PEP440 version", self)
        }
    }
}

/// An error returned when a "version bump scheme" cannot be parsed, or if it is
/// not allowed for the version template. The first inner value is the bump
/// scheme text, and the second inner value is the template version.
#[derive(Debug, ThisError)]
#[error("illegal version-bump scheme \"{0}\" for version template {1:?}")]
pub struct UnsupportedBumpSchemeError(pub String, pub Version);

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

        #[allow(clippy::unnecessary_wraps)]
        fn apply_dev_datecode(version: &mut Version) -> Result<()> {
            let local = Local::now();

            match version {
                Version::Semver(v) => {
                    let code = format!("{:04}{:02}{:02}", local.year(), local.month(), local.day());
                    v.build = semver::BuildMetadata::new(&code).unwrap();
                }

                Version::Pep440(v) => {
                    // Here we use a `dev` series number rather than the `local_identifier` so
                    // that it can be expressed as a version_info tuple if needed.
                    let num = 10000 * (local.year() as usize)
                        + 100 * (local.month() as usize)
                        + (local.day() as usize);
                    v.dev_release = Some(num);
                }

                Version::DotNet(v) => {
                    // We can't use a human-readable date-code because version
                    // terms have a maximum value of 65534, so we use a number
                    // that's about the number of days since 1970. That should
                    // take us to about the year 2149.
                    v.revision = (local.timestamp() / 86400) as i32;
                }
            }

            Ok(())
        }

        #[allow(clippy::unnecessary_wraps)]
        fn apply_micro_bump(version: &mut Version) -> Result<()> {
            match version {
                Version::Semver(v) => {
                    v.pre = semver::Prerelease::EMPTY;
                    v.build = semver::BuildMetadata::EMPTY;
                    v.patch += 1;
                }

                Version::Pep440(v) => {
                    while v.segments.len() < 3 {
                        v.segments.push(0);
                    }

                    v.pre_release = None;
                    v.post_release = None;
                    v.dev_release = None;
                    v.local_identifier = None;

                    v.segments[2] += 1;
                    v.segments.truncate(3);
                }

                Version::DotNet(v) => {
                    v.revision = 0;
                    v.build += 1;
                }
            }

            Ok(())
        }

        #[allow(clippy::unnecessary_wraps)]
        fn apply_minor_bump(version: &mut Version) -> Result<()> {
            match version {
                Version::Semver(v) => {
                    v.pre = semver::Prerelease::EMPTY;
                    v.build = semver::BuildMetadata::EMPTY;
                    v.patch = 0;
                    v.minor += 1;
                }

                Version::Pep440(v) => {
                    while v.segments.len() < 3 {
                        v.segments.push(0);
                    }

                    v.pre_release = None;
                    v.post_release = None;
                    v.dev_release = None;
                    v.local_identifier = None;

                    v.segments[1] += 1;
                    v.segments[2] = 0;
                    v.segments.truncate(3);
                }

                Version::DotNet(v) => {
                    v.revision = 0;
                    v.build = 0;
                    v.minor += 1;
                }
            }

            Ok(())
        }

        #[allow(clippy::unnecessary_wraps)]
        fn apply_major_bump(version: &mut Version) -> Result<()> {
            match version {
                Version::Semver(v) => {
                    v.pre = semver::Prerelease::EMPTY;
                    v.build = semver::BuildMetadata::EMPTY;
                    v.patch = 0;
                    v.minor = 0;
                    v.major += 1;
                }

                Version::Pep440(v) => {
                    while v.segments.len() < 3 {
                        v.segments.push(0);
                    }

                    v.pre_release = None;
                    v.post_release = None;
                    v.dev_release = None;
                    v.local_identifier = None;

                    v.segments[0] += 1;
                    v.segments[1] = 0;
                    v.segments[2] = 0;
                    v.segments.truncate(3);
                }

                Version::DotNet(v) => {
                    v.revision = 0;
                    v.build = 0;
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

/// .NET System.Version versions
mod dotnet {
    use anyhow::bail;
    use std::fmt::{Display, Formatter};

    use crate::errors::{Error, Result};

    /// A version compatible with .NET's System.Version
    ///
    /// These versions are simple: they have the form
    /// `{major}.{minor}.{build}.{revision}`. Each term must be between 0 and
    /// 65534.
    #[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
    pub struct DotNetVersion {
        pub major: i32,
        pub minor: i32,
        pub build: i32,
        pub revision: i32,
    }

    impl Display for DotNetVersion {
        fn fmt(&self, f: &mut Formatter) -> std::result::Result<(), std::fmt::Error> {
            write!(
                f,
                "{}.{}.{}.{}",
                self.major, self.minor, self.build, self.revision
            )
        }
    }

    impl std::str::FromStr for DotNetVersion {
        type Err = Error;

        fn from_str(s: &str) -> Result<Self> {
            let pieces: std::result::Result<Vec<_>, _> = s.split('.').map(|s| s.parse()).collect();

            match pieces.as_ref().map(|v| v.len()) {
                Ok(4) => {}
                _ => bail!("failed to parse `{}` as a .NET version", s),
            }

            let pieces = pieces.unwrap();

            Ok(DotNetVersion {
                major: pieces[0],
                minor: pieces[1],
                build: pieces[2],
                revision: pieces[3],
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn greater_less() {
            const CASES: &[(&str, &str)] = &[
                ("0.0.0.9999", "0.0.1.0"),
                ("0.0.0.9999", "0.1.0.0"),
                ("0.0.0.9999", "1.0.0.0"),
                ("1.0.0.0", "1.0.0.1"),
            ];

            for (l_text, g_text) in CASES {
                let lesser = l_text.parse::<DotNetVersion>().unwrap();
                let greater = g_text.parse::<DotNetVersion>().unwrap();
                assert!(lesser < greater);
                assert!(greater > lesser);
            }
        }
    }
}

/// Python PEP-440 versions.
mod pep440 {
    use anyhow::bail;
    use std::{
        cmp::Ordering,
        fmt::{Display, Formatter},
    };

    use crate::errors::{Error, Result};

    /// A version compatible with the Python PEP-440 specification.
    ///
    /// This structure stores versions in normalized form. You won't necessarily be
    /// able to roundtrip them back to the input text, if the input text is
    /// un-normalized.
    ///
    /// There is a crate named `verlib` that I was hoping to use for this, but it
    /// turns out that it doesn't actually implement PEP440 yet.
    #[derive(Clone, Debug)]
    pub struct Pep440Version {
        pub epoch: usize,
        pub segments: Vec<usize>,
        pub pre_release: Option<Pep440Prerelease>,
        pub post_release: Option<usize>,
        pub dev_release: Option<usize>,
        pub local_identifier: Option<String>,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Pep440Prerelease {
        Alpha(usize),
        Beta(usize),
        Rc(usize),
    }

    impl Pep440Version {
        pub fn parse_from_tuple_literal(s: &str) -> Result<Self> {
            match parse::version_from_tuple_literal(s) {
                Ok((_, v)) => Ok(v),
                Err(e) => bail!(
                    "failed to parse `{}` as a sys.version_info-like tuple literal: {}",
                    s,
                    e
                ),
            }
        }

        pub fn as_tuple_literal(&self) -> Result<String> {
            let major = self.segments[0];

            let minor = if self.segments.len() > 1 {
                self.segments[1]
            } else {
                0
            };

            let micro = if self.segments.len() > 2 {
                self.segments[2]
            } else {
                0
            };

            if self.segments.len() > 3 {
                bail!(
                    "cannot express PEP440 version {} as a version_info tuple",
                    self
                );
            }

            let (pre_code, pre_serial) =
                match (self.pre_release, self.post_release, self.dev_release) {
                    (Some(Pep440Prerelease::Alpha(serial)), None, None) => ("alpha", serial),
                    (Some(Pep440Prerelease::Beta(serial)), None, None) => ("beta", serial),
                    (Some(Pep440Prerelease::Rc(serial)), None, None) => ("candidate", serial),
                    (None, None, None) => ("final", 0),
                    (None, None, Some(serial)) => ("dev", serial),
                    _ => bail!(
                        "cannot express PEP440 version {} as a version_info tuple",
                        self
                    ),
                };

            Ok(format!(
                "({major}, {minor}, {micro}, '{pre_code}', {pre_serial})"
            ))
        }
    }

    impl Default for Pep440Version {
        fn default() -> Self {
            Pep440Version {
                epoch: 0,
                segments: vec![0; 1],
                pre_release: None,
                post_release: None,
                dev_release: None,
                local_identifier: None,
            }
        }
    }

    impl Display for Pep440Version {
        fn fmt(&self, f: &mut Formatter) -> std::result::Result<(), std::fmt::Error> {
            if self.epoch != 0 {
                write!(f, "{}!", self.epoch)?;
            }

            write!(f, "{}", self.segments[0])?;

            for more in &self.segments[1..] {
                write!(f, ".{more}")?;
            }

            if let Some(ref p) = self.pre_release {
                write!(f, ".{p}")?;
            }

            if let Some(n) = self.post_release {
                write!(f, ".post{n}")?;
            }

            if let Some(n) = self.dev_release {
                write!(f, ".dev{n}")?;
            }

            if let Some(ref l) = self.local_identifier {
                write!(f, "+{l}")?;
            }

            Ok(())
        }
    }

    impl Display for Pep440Prerelease {
        fn fmt(&self, f: &mut Formatter) -> std::result::Result<(), std::fmt::Error> {
            match self {
                Pep440Prerelease::Alpha(n) => write!(f, "a{n}"),
                Pep440Prerelease::Beta(n) => write!(f, "b{n}"),
                Pep440Prerelease::Rc(n) => write!(f, "rc{n}"),
            }
        }
    }

    impl std::str::FromStr for Pep440Version {
        type Err = Error;

        fn from_str(s: &str) -> Result<Self> {
            let lower = s.to_lowercase();

            match parse::version(&lower) {
                Ok((_, v)) => Ok(v),
                Err(e) => bail!("failed to parse `{}` as a PEP-440 version: {}", s, e),
            }
        }
    }

    impl std::cmp::Ord for Pep440Version {
        fn cmp(&self, other: &Self) -> Ordering {
            let o = self.epoch.cmp(&other.epoch);
            if o != Ordering::Equal {
                return o;
            }

            // There's probably a cleaner way to deal with differing-length lists ..
            let ns = self.segments.len();
            let no = other.segments.len();

            for i in 0..std::cmp::max(ns, no) {
                let vs = if i < ns { self.segments[i] } else { 0 };
                let vo = if i < no { other.segments[i] } else { 0 };
                let o = vs.cmp(&vo);
                if o != Ordering::Equal {
                    return o;
                }
            }

            let pss = within_release_score(self);
            let pso = within_release_score(other);
            return pss.cmp(&pso);

            /// This function "scores" a version's pre-release-ness. The first
            /// returned value is a number that reflects the overall ranking of
            /// the particular combination of pre/post/dev flags; the remaining
            /// three numbers give the specific values of those flags, ordered
            /// in the appropriate way to allow meaningful comparison if the
            /// pre/post/dev flags are tied.
            fn within_release_score(v: &Pep440Version) -> [usize; 4] {
                match (v.dev_release, v.pre_release, v.post_release) {
                    (Some(dev), None, None) => [100, dev, 0, 0], // .dev

                    (Some(dev), Some(pre), None) => {
                        // .pre .dev
                        let (offset, pre) = prerelease_scores(&pre);
                        [190 + offset, pre, dev, 0]
                    }

                    (None, Some(pre), None) => {
                        // .pre
                        let (offset, pre) = prerelease_scores(&pre);
                        [200 + offset, pre, 0, 0]
                    }

                    (Some(dev), Some(pre), Some(post)) => {
                        // .pre .post .dev
                        let (offset, pre) = prerelease_scores(&pre);
                        [210 + offset, pre, post, dev]
                    }

                    (None, Some(pre), Some(post)) => {
                        // .pre .post
                        let (offset, pre) = prerelease_scores(&pre);
                        [220 + offset, pre, post, 0]
                    }

                    (None, None, None) => [500, 0, 0, 0], // (nothing)
                    (Some(dev), None, Some(post)) => [609, post, dev, 0], // .post .dev
                    (None, None, Some(post)) => [610, post, 0, 0], // .post
                }
            }

            fn prerelease_scores(pr: &Pep440Prerelease) -> (usize, usize) {
                match pr {
                    Pep440Prerelease::Alpha(n) => (0, *n),
                    Pep440Prerelease::Beta(n) => (100, *n),
                    Pep440Prerelease::Rc(n) => (200, *n),
                }
            }
        }
    }

    impl PartialOrd for Pep440Version {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    // Custom implementation to ignore local_identifier, which does not
    // factor into the ordering
    impl PartialEq for Pep440Version {
        fn eq(&self, other: &Self) -> bool {
            if self.epoch != other.epoch
                || self.pre_release != other.pre_release
                || self.dev_release != other.dev_release
                || self.post_release != other.post_release
            {
                return false;
            }

            let ns = self.segments.len();
            let no = other.segments.len();

            for i in 0..std::cmp::max(ns, no) {
                let vs = if i < ns { self.segments[i] } else { 0 };
                let vo = if i < no { other.segments[i] } else { 0 };
                if vs != vo {
                    return false;
                }
            }

            true
        }
    }

    impl Eq for Pep440Version {}

    mod parse {
        use nom::{
            branch::alt,
            bytes::complete::{tag, take_till},
            character::complete::{char, digit1, multispace0, one_of},
            combinator::{all_consuming, map, map_res, opt},
            error::ErrorKind,
            AsChar, IResult, InputTakeAtPosition,
        };

        use super::*;

        /// Parse an unsigned integer.
        fn unsigned(i: &str) -> IResult<&str, usize> {
            map_res(digit1, |s: &str| s.parse::<usize>())(i)
        }

        /// Parse a PEP440 separator: one of ".", "-", or "_"
        fn separator(i: &str) -> IResult<&str, char> {
            one_of(".-_")(i)
        }

        fn not_alpha_or_separator<T: AsChar>(c: T) -> bool {
            let c = c.as_char();
            !(c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
        }

        /// extract a substring of (alphanumeric or separator)
        fn alpha_or_separator(i: &str) -> IResult<&str, &str> {
            i.split_at_position1_complete(not_alpha_or_separator, ErrorKind::AlphaNumeric)
        }

        /// Parse a period and then a number
        fn dot_unsigned(i: &str) -> IResult<&str, usize> {
            let (i, _) = tag(".")(i)?;
            unsigned(i)
        }

        /// Try to parse an epoch.
        fn epoch(i: &str) -> IResult<&str, usize> {
            let (i, n) = unsigned(i)?;
            let (i, _) = tag("!")(i)?;
            Ok((i, n))
        }

        enum Segment {
            Release(usize),
            PreRelease(Pep440Prerelease),
            PostRelease(usize),
            DevRelease(usize),
            LocalIdentifier(String),
        }

        /// Try to parse a "local identifier".
        fn parse_local_identifier(i: &str) -> IResult<&str, Segment> {
            let (i, _) = tag("+")(i)?;
            // TODO: we don't normalize and validate these rigorously right now, but
            // maybe we will later => allocate a String.
            let (i, text) = alpha_or_separator(i)?;
            Ok((i, Segment::LocalIdentifier(text.to_owned())))
        }

        /// Try to parse a development release tag
        fn dev_tag(i: &str) -> IResult<&str, Segment> {
            let (i, _) = opt(separator)(i)?;
            let (i, _) = tag("dev")(i)?;
            let (i, _) = opt(separator)(i)?;
            let (i, n) = map(opt(unsigned), |o| o.unwrap_or(0))(i)?;
            Ok((i, Segment::DevRelease(n)))
        }

        /// Try to parse a post-release that is explicitly tagged
        fn explicit_post_tag(i: &str) -> IResult<&str, Segment> {
            let (i, _) = opt(separator)(i)?;
            let (i, _) = alt((tag("post"), tag("r"), tag("rev")))(i)?;
            let (i, _) = opt(separator)(i)?;
            let (i, n) = map(opt(unsigned), |o| o.unwrap_or(0))(i)?;
            Ok((i, Segment::PostRelease(n)))
        }

        /// Try to parse a prerelease tag
        fn pre_tag(i: &str) -> IResult<&str, Segment> {
            let (i, _) = opt(separator)(i)?;
            // order is important here: when there's a common prefix,
            // the longer item must come first:
            let (i, tag_text) = alt((
                tag("alpha"),
                tag("a"),
                tag("beta"),
                tag("b"),
                tag("c"),
                tag("rc"),
                tag("preview"),
                tag("pre"),
            ))(i)?;
            let (i, _) = opt(separator)(i)?;
            let (i, n) = map(opt(unsigned), |o| o.unwrap_or(0))(i)?;

            let pr = match tag_text {
                "a" | "alpha" => Pep440Prerelease::Alpha(n),
                "b" | "beta" => Pep440Prerelease::Beta(n),
                _ => Pep440Prerelease::Rc(n),
            };

            Ok((i, Segment::PreRelease(pr)))
        }

        /// Try to parse an unlabeled post release, which comes immediately
        /// after the main version numbers.
        fn unlabeled_post_tag(i: &str) -> IResult<&str, Segment> {
            let (i, _) = tag("-")(i)?;
            let (i, n) = unsigned(i)?;
            Ok((i, Segment::PostRelease(n)))
        }

        /// Try to parse a complete PEP440 version.
        pub fn version(i: &str) -> IResult<&str, Pep440Version> {
            let (i, _) = multispace0(i)?;
            let (i, _) = opt(tag("v"))(i)?;
            let (i, epoch) = opt(epoch)(i)?;
            let epoch = epoch.unwrap_or(0);

            let mut segments = Vec::new();
            let mut pre_release = None;
            let mut post_release = None;
            let mut dev_release = None;
            let mut local_identifier = None;

            let (mut i, n) = unsigned(i)?;
            let mut segment = Segment::Release(n);

            loop {
                let (new_i, maybe_new_segment) = match segment {
                    Segment::Release(n) => {
                        segments.push(n);
                        opt(alt((
                            pre_tag,
                            explicit_post_tag,
                            dev_tag,
                            parse_local_identifier,
                            unlabeled_post_tag,
                            map(dot_unsigned, Segment::Release),
                        )))(i)
                    }

                    Segment::PreRelease(n) => {
                        pre_release = Some(n);
                        opt(alt((explicit_post_tag, dev_tag, parse_local_identifier)))(i)
                    }

                    Segment::PostRelease(n) => {
                        post_release = Some(n);
                        opt(alt((dev_tag, parse_local_identifier)))(i)
                    }

                    Segment::DevRelease(n) => {
                        dev_release = Some(n);
                        opt(parse_local_identifier)(i)
                    }

                    Segment::LocalIdentifier(s) => {
                        local_identifier = Some(s);
                        Ok((i, None))
                    }
                }?;

                i = new_i;

                if let Some(s) = maybe_new_segment {
                    segment = s;
                } else {
                    break;
                }
            }

            let (i, _) = all_consuming(multispace0)(i)?;

            Ok((
                i,
                Pep440Version {
                    epoch,
                    segments,
                    pre_release,
                    post_release,
                    dev_release,
                    local_identifier,
                },
            ))
        }

        /// Try to parse a simple version from a `sys.version_info` style tuple
        /// literal. We allow arbitrary leading text, since our expected use
        /// case is to analyze a line extracted from a Python source file.
        pub fn version_from_tuple_literal(i: &str) -> IResult<&str, Pep440Version> {
            let (i, _) = take_till(|c| c == '(')(i)?;
            let (i, _) = tag("(")(i)?;
            let (i, _) = multispace0(i)?;
            let (i, major) = unsigned(i)?;
            let (i, _) = multispace0(i)?;
            let (i, _) = tag(",")(i)?;
            let (i, _) = multispace0(i)?;
            let (i, minor) = unsigned(i)?;
            let (i, _) = multispace0(i)?;
            let (i, _) = tag(",")(i)?;
            let (i, _) = multispace0(i)?;
            let (i, micro) = unsigned(i)?;
            let (i, _) = multispace0(i)?;
            let (i, _) = tag(",")(i)?;
            let (i, _) = multispace0(i)?;
            let (i, delim) = one_of("'\"")(i)?;
            let (i, level) = take_till(|c| c == delim)(i)?;
            let (i, _) = char(delim)(i)?;
            let (i, _) = multispace0(i)?;
            let (i, _) = tag(",")(i)?;
            let (i, _) = multispace0(i)?;
            let (i, serial) = unsigned(i)?;
            let (i, _) = multispace0(i)?;
            let (i, _) = tag(")")(i)?;

            let (pre_release, dev_release) = match level {
                "alpha" => (Some(Pep440Prerelease::Alpha(serial)), None),
                "beta" => (Some(Pep440Prerelease::Beta(serial)), None),
                "candidate" => (Some(Pep440Prerelease::Rc(serial)), None),
                "final" => (None, None),
                "dev" => (None, Some(serial)),
                _ => return Err(nom::Err::Failure((i, ErrorKind::Alt))),
            };

            Ok((
                i,
                Pep440Version {
                    epoch: 0,
                    segments: vec![major, minor, micro],
                    pre_release,
                    dev_release,
                    post_release: None,
                    local_identifier: None,
                },
            ))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct CVers<'a>(
            usize,
            &'a [usize],
            Option<Pep440Prerelease>,
            Option<usize>,
            Option<usize>,
            Option<&'a str>,
        );

        impl<'a> CVers<'a> {
            fn to_owned(&self) -> Pep440Version {
                Pep440Version {
                    epoch: self.0,
                    segments: self.1.to_vec(),
                    pre_release: self.2,
                    post_release: self.3,
                    dev_release: self.4,
                    local_identifier: self.5.map(|s| s.to_owned()),
                }
            }
        }

        #[test]
        fn parse() {
            const PARSE_CASES: &[(&str, CVers<'static>)] = &[
                ("0", CVers(0, &[0], None, None, None, None)),
                ("1.010", CVers(0, &[1, 10], None, None, None, None)),
                ("1.0.dev456", CVers(0, &[1, 0], None, None, Some(456), None)),
                (
                    "1.0a12.dev456",
                    CVers(
                        0,
                        &[1, 0],
                        Some(Pep440Prerelease::Alpha(12)),
                        None,
                        Some(456),
                        None,
                    ),
                ),
                (
                    "1.0b2.post345.dev456",
                    CVers(
                        0,
                        &[1, 0],
                        Some(Pep440Prerelease::Beta(2)),
                        Some(345),
                        Some(456),
                        None,
                    ),
                ),
                (
                    "1.0rc1.dev456",
                    CVers(
                        0,
                        &[1, 0],
                        Some(Pep440Prerelease::Rc(1)),
                        None,
                        Some(456),
                        None,
                    ),
                ),
                (
                    "1.0+abc.5",
                    CVers(0, &[1, 0], None, None, None, Some("abc.5")),
                ),
                ("1.0+5", CVers(0, &[1, 0], None, None, None, Some("5"))),
                ("1!1", CVers(1, &[1], None, None, None, None)),
                (
                    "1RC1",
                    CVers(0, &[1], Some(Pep440Prerelease::Rc(1)), None, None, None),
                ),
                (
                    "1.RC.1",
                    CVers(0, &[1], Some(Pep440Prerelease::Rc(1)), None, None, None),
                ),
                (
                    "1-RC-1",
                    CVers(0, &[1], Some(Pep440Prerelease::Rc(1)), None, None, None),
                ),
                (
                    "1_RC_1",
                    CVers(0, &[1], Some(Pep440Prerelease::Rc(1)), None, None, None),
                ),
                (
                    "  v1_RC_1   ",
                    CVers(0, &[1], Some(Pep440Prerelease::Rc(1)), None, None, None),
                ),
                (
                    "  1_RC_1   ",
                    CVers(0, &[1], Some(Pep440Prerelease::Rc(1)), None, None, None),
                ),
                (
                    "1.0a0",
                    CVers(
                        0,
                        &[1, 0],
                        Some(Pep440Prerelease::Alpha(0)),
                        None,
                        None,
                        None,
                    ),
                ),
                (
                    "1.0alpha0",
                    CVers(
                        0,
                        &[1, 0],
                        Some(Pep440Prerelease::Alpha(0)),
                        None,
                        None,
                        None,
                    ),
                ),
                (
                    "1.0b0",
                    CVers(
                        0,
                        &[1, 0],
                        Some(Pep440Prerelease::Beta(0)),
                        None,
                        None,
                        None,
                    ),
                ),
                (
                    "1.0beta0",
                    CVers(
                        0,
                        &[1, 0],
                        Some(Pep440Prerelease::Beta(0)),
                        None,
                        None,
                        None,
                    ),
                ),
                (
                    "1.0pre0",
                    CVers(0, &[1, 0], Some(Pep440Prerelease::Rc(0)), None, None, None),
                ),
                (
                    "1.0preview0",
                    CVers(0, &[1, 0], Some(Pep440Prerelease::Rc(0)), None, None, None),
                ),
            ];

            for (text, cexp) in PARSE_CASES {
                let expected = cexp.to_owned();
                let observed = text.parse::<Pep440Version>().unwrap();
                assert_eq!(expected, observed);
            }
        }

        #[test]
        fn bad_versions() {
            const BAD_CASES: &[&str] = &["-1", "bad!1.0", "1.dev0.pre0"];

            for text in BAD_CASES {
                assert!(text.parse::<Pep440Version>().is_err());
            }
        }

        #[test]
        fn greater_less() {
            const CASES: &[(&str, &str)] = &[
                ("1.0", "1.1"),
                ("1.0.dev.0", "1.0"),
                ("1.0.dev.0", "1.0a0"),
                ("1.0.alpha.0", "1.0b0"),
                ("1.0-b-0", "1.0c0"),
                ("1.0rc0", "1.0"),
                ("1.0", "1.0.post.0"),
                ("1.0", "1.0-0"),
                ("1.0a0.dev0", "1.0a0"),
                ("1.0a0", "1.0a0.post0"),
                ("1.0b0.dev0", "1.0b0"),
                ("1.0b0", "1.0b0.post0"),
                ("1.0rc0.dev0", "1.0rc0"),
                ("1.0rc0", "1.0rc0.post0"),
                ("1.0.post0.dev0", "1.0.post0"),
                ("1.0rc0", "1.0rc0.post0"),
                ("1.0.b0.post0.dev0", "1.0.b0.post0"),
                ("2020.99", "1!0"),
            ];

            for (l_text, g_text) in CASES {
                let lesser = l_text.parse::<Pep440Version>().unwrap();
                let greater = g_text.parse::<Pep440Version>().unwrap();
                assert!(lesser < greater);
                assert!(greater > lesser);
            }
        }

        #[test]
        fn eq() {
            const CASES: &[(&str, &str)] = &[
                ("1.0", "1"),
                ("1.0.0.0.0", "1"),
                ("1.0+something", "1"),
                ("0!1.0", "1"),
                ("1.0a0", "1.0.alpha.0"),
                ("1.0b0", "1.0-beta-0"),
                ("1.0c0", "1.0rc"),
                ("1.0c0", "1.0pre0"),
                ("1.0c0", "1.0preview"),
                ("1.0-10", "1.0.post_10"),
            ];

            for (l_text, r_text) in CASES {
                let left = l_text.parse::<Pep440Version>().unwrap();
                let right = r_text.parse::<Pep440Version>().unwrap();
                assert_eq!(left, right);
            }
        }

        #[test]
        fn display_roundtrip() {
            const CASES: &[&str] = &["0!0", "1.0.0.0.0.0", "1RC0", "1.0+SOME_TEXT", "1.0-0"];

            for text in CASES {
                let orig = text.parse::<Pep440Version>().unwrap();
                let roundtripped = orig.to_string().parse().unwrap();
                assert_eq!(orig, roundtripped);
            }
        }
    }
}
