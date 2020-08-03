// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Error handline for the CLI application.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("no internal or external subcommand `{0}` is available (install `cranko-{0}`?)")]
    NoSuchSubcommand(String),
}

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

    #[error("{0}")]
    Git(#[from] git2::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("cannot identify the upstream remote for the backing repository")]
    NoUpstreamRemote,

    #[error("reference to resource {0} contained outside of the repository")]
    OutsideOfRepository(String),

    /// Used when our rewriting logic encounters an unexpected file structure,
    /// missing template, etc -- not for I/O errors encountered in process.
    /// E.g., this variant is for when we don't know what to do, not when we try
    /// to do something but it fails.
    #[error("repo rewrite error: {0}")]
    RewriteFormatError(String),

    #[error("TOML format error: {0}")]
    Toml(#[from] toml_edit::TomlError),
}

pub type Result<T> = std::result::Result<T, Error>;
