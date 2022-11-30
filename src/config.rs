// Copyright 2020-2022 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! The Cranko configuration file.
//!
//! Given the same input repository, Cranko should give reproducible results no
//! matter who’s running it. So we really want all configuration to be at the
//! per-repository level.

use anyhow::Context;
use std::{collections::HashMap, fs::File, io::Read, path::Path};

use crate::{
    atry,
    errors::{Error, Result},
};

/// The configuration file structures as explicitly serialized into the TOML
/// format.
mod syntax {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    /// The toplevel (per-repo) configuration structure.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct SerializedConfiguration {
        /// General per-repository configuration.
        pub repo: RepoConfiguration,

        /// NPM integration configuration.
        #[serde(default)]
        pub npm: NpmConfiguration,

        /// Centralized per-project configuration.
        #[serde(default)]
        pub projects: HashMap<String, ProjectConfiguration>,
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

    /// Configuration related to the NPM integration.
    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    pub struct NpmConfiguration {
        /// A custom "resolution protocol" to use for internal dependencies; if
        /// using Yarn workspaces, `"workspace"` may be useful here.
        pub internal_dep_protocol: Option<String>,
    }

    /// Configuration relating to individual projects.
    ///
    /// Whenever possible, this configuration should be specified in per-project
    /// metadata files to preserve locality. But some pieces of configuration
    /// need to be centralized.
    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    pub struct ProjectConfiguration {
        /// Ignore this project if/when it is automatically detected.
        pub ignore: bool,
    }
}

// The rest of this module normalizes the on-disk format into forms more useful
// at runtime.

pub use syntax::{NpmConfiguration, ProjectConfiguration, RepoConfiguration};

#[derive(Clone, Debug)]
pub struct ConfigurationFile {
    pub repo: RepoConfiguration,
    pub npm: NpmConfiguration,
    pub projects: HashMap<String, ProjectConfiguration>,
}

impl Default for ConfigurationFile {
    fn default() -> Self {
        let repo = RepoConfiguration::default();
        let npm = Default::default();
        let projects = Default::default();

        ConfigurationFile {
            repo,
            npm,
            projects,
        }
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

        Ok(ConfigurationFile {
            repo: sercfg.repo,
            npm: sercfg.npm,
            projects: sercfg.projects,
        })
    }

    pub fn into_toml(self) -> Result<String> {
        let syn_cfg = syntax::SerializedConfiguration {
            repo: self.repo,
            npm: self.npm,
            projects: self.projects,
        };
        Ok(atry!(
            toml::to_string_pretty(&syn_cfg);
            ["could not serialize configuration into TOML format"]
        ))
    }
}
