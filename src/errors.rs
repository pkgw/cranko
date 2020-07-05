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
    #[error("{0}")]
    Git(#[from] git2::Error),
}

pub type Result<T> = std::result::Result<T, Error>;