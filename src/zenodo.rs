// Copyright 2022 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Release automation utilities related to the Zenodo service.

use log::info;
use std::path::PathBuf;
use structopt::StructOpt;

use super::Command;
use crate::{app::AppSession, env::require_var, errors::Result};

struct ZenodoInformation {
    token: String,
}

impl ZenodoInformation {
    fn new() -> Result<Self> {
        let token = require_var("ZENODO_TOKEN")?;

        Ok(ZenodoInformation { token })
    }

    fn make_blocking_client(&self) -> Result<reqwest::blocking::Client> {
        use reqwest::header;
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", self.token))?,
        );
        headers.insert(header::USER_AGENT, header::HeaderValue::from_str("cranko")?);

        Ok(reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()?)
    }

    #[allow(unused)]
    fn api_url(&self, rest: &str) -> String {
        format!("https://zenodo.org/api/{}", rest)
    }
}

/// The `zenodo` subcommands.
#[derive(Debug, PartialEq, StructOpt)]
pub enum ZenodoCommands {
    #[structopt(name = "upload-artifacts")]
    /// Upload one or more files as artifacts associated with a Zenodo deposit.
    UploadArtifacts(UploadArtifactsCommand),
}

#[derive(Debug, PartialEq, StructOpt)]
pub struct ZenodoCommand {
    #[structopt(subcommand)]
    command: ZenodoCommands,
}

impl Command for ZenodoCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            ZenodoCommands::UploadArtifacts(o) => o.execute(),
        }
    }
}

/// Upload one or more files as artifacts associated with a Zenodo deposit.
#[derive(Debug, PartialEq, StructOpt)]
pub struct UploadArtifactsCommand {
    #[structopt(help = "The path(s) to the file(s) to upload", required = true)]
    paths: Vec<PathBuf>,
}

impl Command for UploadArtifactsCommand {
    fn execute(self) -> Result<i32> {
        let _sess = AppSession::initialize_default()?;
        let info = ZenodoInformation::new()?;
        let mut _client = info.make_blocking_client()?;
        info!("NOOP");
        Ok(0)
    }
}
