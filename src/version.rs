// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Version numbers.

use chrono::{offset::Local, Datelike};
use std::fmt::{Display, Formatter};
use thiserror::Error as ThisError;

use crate::errors::Result;

/// A version number associated with a project.
///
/// This is an enumeration because different kinds of projects may subscribe to
/// different kinds of versioning schemes.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd)]
pub enum Version {
    /// A version compatible with the semantic versioning specification.
    Semver(semver::Version),
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter) -> std::result::Result<(), std::fmt::Error> {
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

    /// Mutate this version to be Cranko's default "development mode" value.
    pub fn set_to_dev_value(&mut self) {
        match self {
            Version::Semver(v) => {
                v.major = 0;
                v.minor = 0;
                v.patch = 0;
                v.pre.clear();
                v.pre
                    .push(semver::Identifier::AlphaNumeric("dev".to_string()));
                v.pre.push(semver::Identifier::Numeric(0));
                v.build.clear();
            }
        }
    }

    /// Given a template version, parse a "bump scheme" from a textual
    /// description.
    ///
    /// Not all bump schemes are compatible with all versioning styles, which is
    /// why this operation depends on the version template and is fallible.
    pub fn parse_bump_scheme(
        &self,
        text: &str,
    ) -> std::result::Result<VersionBumpScheme, UnsupportedBumpSchemeError> {
        if text.starts_with("force ") {
            return Ok(VersionBumpScheme::Force(text[6..].to_owned()));
        }

        match text {
            "micro bump" => Ok(VersionBumpScheme::MicroBump),
            "minor bump" => Ok(VersionBumpScheme::MinorBump),
            "major bump" => Ok(VersionBumpScheme::MajorBump),
            "dev-datecode" => Ok(VersionBumpScheme::DevDatecode),
            _ => Err(UnsupportedBumpSchemeError(text.to_owned(), self.clone()).into()),
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
    #[derive(Clone, Debug, Eq, PartialEq)]
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

    impl Display for Pep440Version {
        fn fmt(&self, f: &mut Formatter) -> std::result::Result<(), std::fmt::Error> {
            if self.epoch != 0 {
                write!(f, "{}!", self.epoch)?;
            }

            write!(f, "{}", self.segments[0])?;

            for more in &self.segments[1..] {
                write!(f, ".{}", more)?;
            }

            if let Some(ref p) = self.pre_release {
                write!(f, ".{}", p)?;
            }

            if let Some(n) = self.post_release {
                write!(f, ".post{}", n)?;
            }

            if let Some(n) = self.dev_release {
                write!(f, ".dev{}", n)?;
            }

            if let Some(ref l) = self.local_identifier {
                write!(f, "+{}", l)?;
            }

            Ok(())
        }
    }

    impl Display for Pep440Prerelease {
        fn fmt(&self, f: &mut Formatter) -> std::result::Result<(), std::fmt::Error> {
            match self {
                Pep440Prerelease::Alpha(n) => write!(f, "a{}", n),
                Pep440Prerelease::Beta(n) => write!(f, "b{}", n),
                Pep440Prerelease::Rc(n) => write!(f, "rc{}", n),
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

    mod parse {
        use nom::{
            branch::alt,
            bytes::complete::tag,
            character::complete::{digit1, multispace0, one_of},
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
            let (i, tag_text) = alt((
                tag("a"),
                tag("alpha"),
                tag("b"),
                tag("beta"),
                tag("c"),
                tag("rc"),
                tag("pre"),
                tag("preview"),
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
                            map(dot_unsigned, |n| Segment::Release(n)),
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
                    segments: self.1.iter().copied().collect(),
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
                assert_eq!(text.parse::<Pep440Version>().is_err(), true);
            }
        }
    }
}
