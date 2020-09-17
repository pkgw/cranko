// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Error handling for Cranko.

/// The generic error type, for complex operations that can fail for a wide
/// range of reasons. This type is a reexport of the `anyhow` 1.x series Error
/// type. There is an appeal to not explicitly committing ourselves to using
/// this particular error implementation, but the `anyhow` error type has a
/// sufficient number of special methods and traits that it would be pretty
/// tedious to re-implement them all while pretending that we're using some
/// different type.
pub use anyhow::Error;

/// A preloaded result type, in which the error type is our generic error type.
pub type Result<T> = std::result::Result<T, Error>;
