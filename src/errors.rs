// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Error handling for the CLI application.
//!
//! **Note** this enum approach is not great and leaks all sorts of
//! implementation details. I need to go through and clean it up.

use thiserror::Error;

use crate::version::Version;

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum Error {
    #[error("operation on bare repositories is not allowed")]
    BareRepository,

    #[error("{0}")]
    CargoMetadata(#[from] cargo_metadata::Error),

    #[error("internal dependency cycle associated with project {0}")]
    Cycle(String),

    #[error("cannot proceed with a dirty backing repository (path: {0})")]
    DirtyRepository(String),

    // See comment in From implementation below
    #[error("templating error: {0}")]
    Dynfmt(String),

    /// A generic error having to do with the program's execution environment.
    #[error("{0}")]
    Environment(String),

    #[error("error with the backing repository")]
    Git(#[from] git2::Error),

    #[error("{0}")]
    HttpHeader(#[from] reqwest::header::InvalidHeaderValue),

    #[error("invalid \"rc\" changelog format in `{0}`")]
    InvalidChangelogFormat(String),

    #[error("invalid commit message format")]
    InvalidCommitMessageFormat,

    #[error("invalid commit reference `{0}`")]
    InvalidCommitReference(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] json::Error),

    #[error("multiple projects with same name {0} (?!)")]
    NamingClash(String),

    #[error("no such project `{0}`")]
    NoSuchProject(String),

    #[error("data are not parseable as Unicode")]
    NotUnicodeError,

    #[error("cannot identify the upstream remote for the backing repository")]
    NoUpstreamRemote,

    #[error("reference to resource {0} contained outside of the repository")]
    OutsideOfRepository(String),

    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),

    /// Used when our rewriting logic encounters an unexpected file structure,
    /// missing template, etc -- not for I/O errors encountered in process.
    /// E.g., this variant is for when we don't know what to do, not when we try
    /// to do something but it fails.
    #[error("repo rewrite error: {0}")]
    RewriteFormatError(String),

    #[error("{0}")]
    Semver(#[from] semver::SemVerError),

    #[error("TOML deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("TOML format error: {0}")]
    TomlEdit(#[from] toml_edit::TomlError),

    #[error("unsatisfied internal requirement: `{0}` needs newer `{1}`")]
    UnsatisfiedInternalRequirement(String, String),

    #[error("unsupported version-bump scheme \"{0}\" for version template {1:?}")]
    UnsupportedBumpScheme(String, Version),
}

// Note: we cannot preserve the dynfmt::Error since it has an associated
// lifetime. So, we stringify it.
impl<'a> From<dynfmt::Error<'a>> for Error {
    fn from(e: dynfmt::Error<'a>) -> Self {
        Error::Dynfmt(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
