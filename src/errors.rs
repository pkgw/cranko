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

    #[error("{0}")]
    Git(#[from] git2::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("cannot identify the upstream remote for the backing repository")]
    NoUpstreamRemote,

    #[error("TOML format error: {0}")]
    Toml(#[from] toml_edit::TomlError),
}

pub type Result<T> = std::result::Result<T, Error>;
