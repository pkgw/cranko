// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Error handling for the CLI application.
//!
//! **Note** this enum approach is not great and leaks all sorts of
//! implementation details. I need to go through and clean it up.

/// The generic error type, for complex operations that can fail for a wide
/// range of reasons. This type is a reexport of the `anyhow` 1.x series Error
/// type. There is an appeal to not explicitly committing ourselves to using
/// this particular error implementation, but the `anyhow` error type has a
/// sufficient number of special methods and traits that it would be pretty
/// tedious to re-implement them all while pretending that we're using some
/// different type.
pub use anyhow::Error;

use thiserror::Error as ThisError;

use crate::version::Version;

#[non_exhaustive]
#[derive(Debug, ThisError)]
pub enum OldError {
    #[error("reference to resource {0} contained outside of the repository")]
    OutsideOfRepository(String),

    /// Used when our rewriting logic encounters an unexpected file structure,
    /// missing template, etc -- not for I/O errors encountered in process.
    /// E.g., this variant is for when we don't know what to do, not when we try
    /// to do something but it fails.
    #[error("repo rewrite error: {0}")]
    RewriteFormatError(String),

    #[error("unsatisfied internal requirement: `{0}` needs newer `{1}`")]
    UnsatisfiedInternalRequirement(String, String),

    #[error("unsupported version-bump scheme \"{0}\" for version template {1:?}")]
    UnsupportedBumpScheme(String, Version),
}

pub type Result<T> = std::result::Result<T, Error>;
