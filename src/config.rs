// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! The Cranko configuration file.
//!
//! Given the same input repository, Cranko should give reproducible results no
//! matter who’s running it. So we really want all configuration to be at the
//! per-repository level.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};

use crate::errors::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SerializedConfiguration {
    /// General per-repository configuration.
    pub repo: RepoConfiguration,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RepoConfiguration {
    /// Git URLs that the upstream remote might be using.
    pub upstream_urls: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ConfigurationFile {
    pub repo: RepoConfiguration,
}

impl Default for ConfigurationFile {
    fn default() -> Self {
        let repo = RepoConfiguration::default();

        ConfigurationFile { repo }
    }
}

impl ConfigurationFile {
    pub fn get<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut f = match File::open(&path) {
            Ok(f) => f,

            Err(e) => {
                return if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(Self::default())
                } else {
                    Err(Error::new(e).context(format!(
                        "failed to open config file `{}`",
                        path.as_ref().display()
                    )))
                }
            }
        };

        let mut text = String::new();
        f.read_to_string(&mut text)
            .with_context(|| format!("failed to read config file `{}`", path.as_ref().display()))?;

        let sercfg: SerializedConfiguration = toml::from_str(&text).with_context(|| {
            format!(
                "could not parse config file `{}` as TOML",
                path.as_ref().display()
            )
        })?;

        Ok(ConfigurationFile { repo: sercfg.repo })
    }
}
