// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! The Cranko configuration file.
//!
//! Given the same input repository, Cranko should give reproducible results no
//! matter who’s running it. So we really want all configuration to be at the
//! per-repository level.

use anyhow::Context;
use std::{fs::File, io::Read, path::Path};

use crate::{
    atry,
    errors::{Error, Result},
};

/// The configuration file structures as explicitly serialized into the TOML
/// format.
mod syntax {
    use serde::{Deserialize, Serialize};

    /// The toplevel (per-repo) configuration structure.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct SerializedConfiguration {
        /// General per-repository configuration.
        pub repo: RepoConfiguration,
    }

    /// Configuration relating to the backing repository. This is applied
    /// directly to the runtime Repository instance.
    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    pub struct RepoConfiguration {
        /// Git URLs that the upstream remote might be using.
        pub upstream_urls: Vec<String>,

        /// The name of the `rc`-like branch.
        pub rc_name: Option<String>,

        /// The name of the `release`-like branch.
        pub release_name: Option<String>,

        /// The format for release tag names.
        pub release_tag_name_format: Option<String>,
    }
}

// The rest of this module normalizes the on-disk format into forms more useful
// at runtime.

pub use syntax::RepoConfiguration;

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

        let sercfg: syntax::SerializedConfiguration = toml::from_str(&text).with_context(|| {
            format!(
                "could not parse config file `{}` as TOML",
                path.as_ref().display()
            )
        })?;

        Ok(ConfigurationFile { repo: sercfg.repo })
    }

    pub fn into_toml(self) -> Result<String> {
        let syn_cfg = syntax::SerializedConfiguration { repo: self.repo };
        Ok(atry!(
            toml::to_string_pretty(&syn_cfg);
            ["could not serialize configuration into TOML format"]
        ))
    }
}
