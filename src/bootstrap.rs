// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Boostrapping Cranko on a preexisting repository.

use anyhow::bail;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Write};
use structopt::StructOpt;

use super::Command;
use crate::{
    atry,
    errors::{Error, Result},
    project::DepRequirement,
};

/// The toplevel bootstrap state structure.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct BootstrapConfiguration {
    pub project: Vec<BootstrapProjectInfo>,
}

/// Bootstrap info for a project.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BootstrapProjectInfo {
    pub qnames: Vec<String>,
    pub version: String,
    pub release_commit: Option<String>,
}

/// The `bootstrap` commands.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct BootstrapCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force operation even in unexpected conditions"
    )]
    force: bool,

    #[structopt(
        short = "u",
        long = "upstream",
        help = "The name of the Git upstream remote"
    )]
    upstream_name: Option<String>,
}

impl Command for BootstrapCommand {
    fn execute(self) -> Result<i32> {
        info!(
            "bootstrapping with Cranko version {}",
            env!("CARGO_PKG_VERSION")
        );

        // Early business: get the repo and identify the upstream.

        let mut repo = atry!(
            crate::repository::Repository::open_from_env();
            ["Cranko is not being run from a Git working directory"]
            (note "run the bootstrap stage inside the Git work tree that you wish to bootstrap")
        );

        let upstream_url = atry!(
            repo.bootstrap_upstream(self.upstream_name.as_ref().map(|s| s.as_ref()));
            ["Cranko cannot identify the Git upstream URL"]
            (note "use the `--upstream` option to manually identify the upstream Git remote")
        );

        info!("the Git upstream URL is: {}", upstream_url);

        if let Some(dirty) = atry!(
            repo.check_if_dirty(&[]);
            ["failed to check the repository for modified files"]
        ) {
            warn!(
                "bootstrapping with uncommitted changes in the repository (e.g.: `{}`)",
                dirty.escaped()
            );
            if !self.force {
                bail!("refusing to proceed (use `--force` to override)");
            }
        }

        // Stub the config file.

        {
            let mut cfg = crate::config::ConfigurationFile::default();
            cfg.repo.upstream_urls = vec![upstream_url];
            let cfg_text = cfg.into_toml()?;

            let mut cfg_path = repo.resolve_config_dir();
            atry!(
                fs::create_dir_all(&cfg_path);
                ["could not create Cranko configuration directory `{}`", cfg_path.display()]
            );

            cfg_path.push("config.toml");
            info!(
                "stubbing Cranko configuration file `{}`",
                cfg_path.display(),
            );

            let f = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&cfg_path)
            {
                Ok(f) => Some(f),
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        warn!(
                            "Cranko configuration file `{}` already exists; not modifying it",
                            cfg_path.display()
                        );
                        None
                    } else {
                        return Err(Error::new(e).context(format!(
                            "failed to open Cranko configuration file `{}` for writing",
                            cfg_path.display()
                        )));
                    }
                }
            };

            if let Some(mut f) = f {
                atry!(
                    f.write_all(cfg_text.as_bytes());
                    ["could not write Cranko configuration file `{}`", cfg_path.display()]
                );
            }
        }

        // Now we can initialize the regular app and report on the projects.

        let mut sess = atry!(
            crate::app::AppSession::initialize_default();
            ["could not initialize app and project graph"]
        );

        let mut seen_any = false;

        for ident in sess.graph().toposorted() {
            let proj = sess.graph().lookup(ident);

            if !seen_any {
                info!("Cranko detected the following projects in the repo:");
                println!();
                seen_any = true;
            }

            let loc_desc = {
                let p = proj.prefix();

                if p.len() == 0 {
                    "the root directory".to_owned()
                } else {
                    format!("`{}`", p.escaped())
                }
            };

            println!(
                "    {} @ {} in {}",
                proj.user_facing_name, proj.version, loc_desc
            );
        }

        if seen_any {
            println!();
            info!("consult the documentation if these results are unexpected");
            info!("autodetection letting you down? file an issue: https://github.com/pkgw/cranko/issues/new");
        } else {
            error!("Cranko failed to discover any projects in the repo");
            error!("autodetection letting you down? file an issue: https://github.com/pkgw/cranko/issues/new");
            return Ok(1);
        }

        // Reset internal version specifications. Bit hacky: first, zero out all
        // versions and rewrite metafiles with exact dev-mode interdependencies.

        let mut bs_cfg = BootstrapConfiguration::default();
        let mut versions = HashMap::new();

        for proj in sess.graph_mut().toposorted_mut() {
            bs_cfg.project.push(BootstrapProjectInfo {
                qnames: proj.qualified_names().to_owned(),
                version: proj.version.to_string(),
                release_commit: None,
            });

            proj.version.set_to_dev_value();
            versions.insert(proj.ident(), proj.version.clone());

            for dep in &mut proj.internal_deps[..] {
                // By definition of the toposort, the version will always be avilable.
                dep.cranko_requirement = DepRequirement::Manual(versions[&dep.ident].to_string());
            }
        }

        // Save the old versions to the bootstrap file.

        let bs_text = atry!(
            toml::to_string_pretty(&bs_cfg);
            ["could not serialize bootstrap data into TOML format"]
        );

        {
            let mut bs_path = repo.resolve_config_dir();
            bs_path.push("bootstrap.toml");
            info!("writing versioning bootstrap file `{}`", bs_path.display());

            let mut f = atry!(
                fs::OpenOptions::new().write(true).create_new(true).open(&bs_path);
                ["could not create bootstrap file `{}`", bs_path.display()]
            );
            atry!(
                f.write_all(bs_text.as_bytes());
                ["could not write bootstrap file `{}`", bs_path.display()]
            );
        }

        // Rewrite the project files with the zeroed versions.

        info!("updating project meta-files with developer versions");

        let changes = atry!(
            sess.rewrite();
            ["there was a problem updating the project files"]
        );

        let mut seen_any = false;

        for path in changes.paths() {
            if !seen_any {
                info!("modified:");
                println!();
                seen_any = true;
            }

            println!("    {}", path.escaped());
        }

        if seen_any {
            println!();
        } else {
            info!("... no files modified. This might be OK.")
        }

        // Now, re-rewrite the internal dependency specifications to contain
        // whatever requirements were listed before, and rewrite only those
        // portions of the metafiles. Note that our processing above will not
        // have altered `dep.literal`.

        for proj in sess.graph_mut().toposorted_mut() {
            for dep in &mut proj.internal_deps[..] {
                dep.cranko_requirement = DepRequirement::Manual(dep.literal.clone());
            }
        }

        atry!(
            sess.rewrite_cranko_requirements();
            ["there was a problem adding Cranko dependency metadata to the project files"]
        );

        // All done.
        info!("modifications complete!");
        println!();
        info!("Review changes, add `.config/cranko/` to the repository, and commit.");
        info!("Then try `cranko status` for a history summary");
        info!("   (its results will be imprecise because Cranko cannot trace into pre-Cranko history)");
        info!("Then begin modifying your CI/CD pipeline to use the `cranko release-workflow` commands");
        Ok(0)
    }
}
