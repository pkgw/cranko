// Copyright 2020-2022 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Release automation utilities related to the GitHub service.

use anyhow::{anyhow, Context};
use json::{object, JsonValue};
use log::{error, info, warn};
use std::{fs::File, path::PathBuf};
use structopt::StructOpt;

use super::Command;
use crate::{
    app::{AppBuilder, AppSession},
    env::require_var,
    errors::Result,
    graph,
    project::Project,
    repository::{CommitId, ReleasedProjectInfo},
};

struct GitHubInformation {
    slug: String,
    token: String,
}

impl GitHubInformation {
    fn new(sess: &AppSession) -> Result<Self> {
        let token = require_var("GITHUB_TOKEN")?;

        let upstream_url = sess.repo.upstream_url()?;
        info!("upstream url: {}", upstream_url);

        let upstream_url = git_url_parse::GitUrl::parse(&upstream_url)
            .map_err(|e| anyhow!("cannot parse upstream Git URL `{}`: {}", upstream_url, e))?;

        let slug = upstream_url.fullname;

        Ok(GitHubInformation { slug, token })
    }

    fn make_blocking_client(&self) -> Result<reqwest::blocking::Client> {
        use reqwest::header;
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("token {}", self.token))?,
        );
        headers.insert(header::USER_AGENT, header::HeaderValue::from_str("cranko")?);

        Ok(reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()?)
    }

    fn api_url(&self, rest: &str) -> String {
        format!("https://api.github.com/repos/{}/{}", self.slug, rest)
    }

    /// Delete an existing release.
    fn delete_release(&self, tag_name: &str, client: &mut reqwest::blocking::Client) -> Result<()> {
        let query_url = self.api_url(&format!("releases/tags/{tag_name}"));

        let resp = client.get(query_url).send()?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "no GitHub release for tag `{}`: {}",
                tag_name,
                resp.text()
                    .unwrap_or_else(|_| "[non-textual server response]".to_owned())
            ));
        }

        let metadata = json::parse(&resp.text()?)?;
        let id = metadata["id"].to_string();

        let delete_url = self.api_url(&format!("releases/{id}"));
        let resp = client.delete(delete_url).send()?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "could not delete GitHub release for tag `{}`: {}",
                tag_name,
                resp.text()
                    .unwrap_or_else(|_| "[non-textual server response]".to_owned())
            ));
        }

        Ok(())
    }

    /// Get information about an existing release by its tag name.
    fn get_custom_release_metadata(
        &self,
        tag_name: &str,
        client: &mut reqwest::blocking::Client,
    ) -> Result<JsonValue> {
        let query_url = self.api_url(&format!("releases/tags/{tag_name}"));

        let resp = client.get(query_url).send()?;
        if resp.status().is_success() {
            Ok(json::parse(&resp.text()?)?)
        } else {
            Err(anyhow!(
                "no GitHub release for tag `{}`: {}",
                tag_name,
                resp.text()
                    .unwrap_or_else(|_| "[non-textual server response]".to_owned())
            ))
        }
    }

    /// Get information about an existing release of a project.
    fn get_release_metadata(
        &self,
        sess: &AppSession,
        proj: &Project,
        rel: &ReleasedProjectInfo,
        client: &mut reqwest::blocking::Client,
    ) -> Result<JsonValue> {
        let tag_name = sess.repo.get_tag_name(proj, rel)?;
        self.get_custom_release_metadata(&tag_name, client)
    }

    /// Create a new GitHub release.
    fn create_custom_release(
        &self,
        tag_name: String,
        release_name: String,
        body: String,
        is_draft: bool,
        is_prerelease: bool,
        client: &mut reqwest::blocking::Client,
    ) -> Result<JsonValue> {
        let saved_tag_name = tag_name.clone();
        let release_info = object! {
            "tag_name" => tag_name,
            "name" => release_name,
            "body" => body,
            "draft" => is_draft,
            "prerelease" => is_prerelease,
        };

        let create_url = self.api_url("releases");
        let resp = client
            .post(create_url)
            .body(json::stringify(release_info))
            .send()?;
        let status = resp.status();
        let parsed = json::parse(&resp.text()?)?;

        if status.is_success() {
            info!("created GitHub release for {}", saved_tag_name);
            Ok(parsed)
        } else {
            Err(anyhow!(
                "failed to create GitHub release for {}: {}",
                saved_tag_name,
                parsed
            ))
        }
    }

    /// Create a new GitHub release.
    fn create_release(
        &self,
        sess: &AppSession,
        proj: &Project,
        rel: &ReleasedProjectInfo,
        cid: &CommitId,
        client: &mut reqwest::blocking::Client,
    ) -> Result<JsonValue> {
        let tag_name = sess.repo.get_tag_name(proj, rel)?;
        let release_name = format!("{} {}", proj.user_facing_name, proj.version);
        let changelog = proj.changelog.scan_changelog(proj, &sess.repo, cid)?;
        self.create_custom_release(tag_name, release_name, changelog, false, false, client)
    }
}

/// The `github` subcommands.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub enum GithubCommands {
    #[structopt(name = "create-custom-release")]
    /// Create a single, customized GitHub release
    CreateCustomRelease(CreateCustomReleaseCommand),

    #[structopt(name = "create-releases")]
    /// Create one or more new GitHub releases
    CreateReleases(CreateReleasesCommand),

    #[structopt(name = "_credential-helper", setting = structopt::clap::AppSettings::Hidden)]
    /// (hidden) github credential helper
    CredentialHelper(CredentialHelperCommand),

    #[structopt(name = "delete-release")]
    /// Delete an existing GitHub release
    DeleteRelease(DeleteReleaseCommand),

    #[structopt(name = "install-credential-helper")]
    /// Install Cranko as a Git "credential helper", using $GITHUB_TOKEN to log in
    InstallCredentialHelper(InstallCredentialHelperCommand),

    #[structopt(name = "upload-artifacts")]
    /// Upload one or more files as GitHub release artifacts
    UploadArtifacts(UploadArtifactsCommand),
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct GithubCommand {
    #[structopt(subcommand)]
    command: GithubCommands,
}

impl Command for GithubCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            GithubCommands::CreateCustomRelease(o) => o.execute(),
            GithubCommands::CreateReleases(o) => o.execute(),
            GithubCommands::CredentialHelper(o) => o.execute(),
            GithubCommands::DeleteRelease(o) => o.execute(),
            GithubCommands::InstallCredentialHelper(o) => o.execute(),
            GithubCommands::UploadArtifacts(o) => o.execute(),
        }
    }
}

/// Create a single custom GitHub release.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct CreateCustomReleaseCommand {
    #[structopt(long = "name", help = "The user-facing name for the release")]
    release_name: String,

    #[structopt(
        long = "desc",
        help = "The release description text (Markdown-formatted)",
        default_value = "Release automatically created by Cranko."
    )]
    body: String,

    #[structopt(long = "draft", help = "Whether to mark this release as a draft")]
    is_draft: bool,

    #[structopt(
        long = "prerelease",
        help = "Whether to mark this release as a pre-release"
    )]
    is_prerelease: bool,

    #[structopt(help = "Name of the Git(Hub) tag to use as the release basis")]
    tag_name: String,
}

impl Command for CreateCustomReleaseCommand {
    fn execute(self) -> Result<i32> {
        let sess = AppBuilder::new()?.populate_graph(false).initialize()?;
        let info = GitHubInformation::new(&sess)?;
        let mut client = info.make_blocking_client()?;
        info.create_custom_release(
            self.tag_name,
            self.release_name,
            self.body,
            self.is_draft,
            self.is_prerelease,
            &mut client,
        )?;
        Ok(0)
    }
}

/// Create new release(s) on GitHub.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct CreateReleasesCommand {
    #[structopt(help = "Name(s) of the project(s) to release on GitHub")]
    proj_names: Vec<String>,
}

impl Command for CreateReleasesCommand {
    fn execute(self) -> Result<i32> {
        let sess = AppSession::initialize_default()?;
        let info = GitHubInformation::new(&sess)?;

        let (dev_mode, rel_info) = sess.ensure_ci_release_mode()?;
        let rel_commit = rel_info
            .commit
            .as_ref()
            .ok_or_else(|| anyhow!("no commit ID for HEAD (?)"))?;

        if dev_mode {
            return Err(anyhow!("refusing to proceed in dev mode"));
        }

        // Get the list of projects that we're interested in.
        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let no_names = q.no_names();
        let idents = sess
            .graph()
            .query(q)
            .context("could not select projects for GitHub release")?;

        if idents.is_empty() {
            info!("no projects selected");
            return Ok(0);
        }

        let mut client = info.make_blocking_client()?;
        let mut n_released = 0;

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);

            if let Some(rel) = rel_info.lookup_if_released(proj) {
                info.create_release(&sess, proj, rel, rel_commit, &mut client)?;
                n_released += 1;
            } else if !no_names {
                warn!(
                    "project {} was specified but does not have a new release",
                    proj.user_facing_name
                );
            }
        }

        if no_names && n_released != 1 {
            info!(
                "created GitHub releases for {} of {} projects",
                n_released,
                idents.len()
            );
        } else if n_released != idents.len() {
            warn!(
                "created GitHub releases for {} of {} selected projects",
                n_released,
                idents.len()
            );
        }

        Ok(0)
    }
}

/// hidden Git credential helper command
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct CredentialHelperCommand {
    #[structopt(help = "The operation")]
    operation: String,
}

impl Command for CredentialHelperCommand {
    fn execute(self) -> Result<i32> {
        if self.operation != "get" {
            info!("ignoring Git credential operation `{}`", self.operation);
        } else {
            let token = require_var("GITHUB_TOKEN")?;
            println!("username=token");
            println!("password={token}");
        }

        Ok(0)
    }
}

/// Delete a release from GitHub.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct DeleteReleaseCommand {
    #[structopt(help = "Name of the release's tag on GitHub")]
    tag_name: String,
}

impl Command for DeleteReleaseCommand {
    fn execute(self) -> Result<i32> {
        let sess = AppSession::initialize_default()?;
        let info = GitHubInformation::new(&sess)?;
        let mut client = info.make_blocking_client()?;
        info.delete_release(&self.tag_name, &mut client)?;
        info!(
            "deleted GitHub release associated with tag `{}`",
            self.tag_name
        );
        Ok(0)
    }
}

/// Install as a Git credential helper
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct InstallCredentialHelperCommand {}

impl Command for InstallCredentialHelperCommand {
    fn execute(self) -> Result<i32> {
        // The path given to Git must be an absolute path.
        let this_exe = std::env::current_exe()?;
        let this_exe = this_exe.to_str().ok_or_else(|| {
            anyhow!(
                "cannot install cranko as a Git \
                 credential helper because its executable path is not Unicode"
            )
        })?;
        let mut cfg = git2::Config::open_default().context("cannot open Git configuration")?;
        cfg.set_str(
            "credential.helper",
            &format!("{this_exe} github _credential-helper"),
        )
        .context("cannot update Git configuration setting `credential.helper`")?;
        Ok(0)
    }
}

/// Upload one or more artifact files to a GitHub release.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct UploadArtifactsCommand {
    #[structopt(
        long = "overwrite",
        help = "Overwrite artifacts if they already exist in the release (default: error out)"
    )]
    overwrite: bool,

    #[structopt(
        long = "by-tag",
        help = "Identify the target release by Git tag name, not Cranko project name"
    )]
    by_tag: bool,

    #[structopt(help = "The released project or tag for which to upload content")]
    proj_name: String,

    #[structopt(help = "The path(s) to the file(s) to upload", required = true)]
    paths: Vec<PathBuf>,
}

impl Command for UploadArtifactsCommand {
    fn execute(self) -> Result<i32> {
        let sess = AppSession::initialize_default()?;
        let info = GitHubInformation::new(&sess)?;
        let mut client = info.make_blocking_client()?;

        let mut metadata = if self.by_tag {
            info.get_custom_release_metadata(&self.proj_name, &mut client)
        } else {
            let rel_info = sess.repo.parse_release_info_from_head().context(
                "expected Cranko release metadata in the HEAD commit but could not load it",
            )?;

            let ident = sess
                .graph()
                .lookup_ident(&self.proj_name)
                .ok_or_else(|| anyhow!("no such project `{}`", self.proj_name))?;

            let rel = rel_info
                .lookup_if_released(sess.graph().lookup(ident))
                .ok_or_else(|| {
                    anyhow!(
                        "project `{}` does not seem to be freshly released",
                        self.proj_name
                    )
                })?;

            // Get information about the release

            let proj = sess.graph().lookup(ident);
            info.get_release_metadata(&sess, proj, rel, &mut client)
        }?;

        let upload_url = metadata["upload_url"]
            .take_string()
            .ok_or_else(|| anyhow!("no upload_url in release metadata?"))?;
        let upload_url = {
            // The returned value includes template `{?name,label}` at the end.
            let v: Vec<&str> = upload_url.split('{').collect();
            v[0].to_owned()
        };

        info!("upload url = {}", upload_url);

        // Upload artifacts

        for path in &self.paths {
            // Make sure the file exists!
            let file = File::open(path)?;

            let name = path
                .file_name()
                .ok_or_else(|| anyhow!("input file has no name component??"))?
                .to_str()
                .ok_or_else(|| anyhow!("input file name cannot be stringified"))?
                .to_owned();

            // If we're in overwrite mode, delete the artifact if it already
            // exists. This is racy, but the API doesn't give us a better method.

            if self.overwrite {
                for asset_info in metadata["assets"].members() {
                    // The `json` docs make it seem like I should just be able to
                    // write `asset_info["name"] == name`, but empirically that's
                    // not working.
                    if asset_info["name"].as_str() == Some(&name) {
                        info!("deleting preexisting asset (id {})", asset_info["id"]);

                        let del_url =
                            info.api_url(&format!("releases/assets/{}", asset_info["id"]));
                        let resp = client.delete(&del_url).send()?;
                        let status = resp.status();

                        if !status.is_success() {
                            error!("API response: {}", resp.text()?);
                            return Err(anyhow!("deletion of pre-existing asset {} failed", name));
                        }
                    }
                }
            }

            // Ready to upload now.

            info!("uploading {} => {}", path.display(), name);
            let url = reqwest::Url::parse_with_params(&upload_url, &[("name", &name)])?;
            let resp = client
                .post(url)
                .header(
                    reqwest::header::ACCEPT,
                    "application/vnd.github.manifold-preview",
                )
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(file)
                .send()?;
            let status = resp.status();
            let mut parsed = json::parse(&resp.text()?)?;

            if !status.is_success() {
                error!("API response: {}", parsed);
                return Err(anyhow!("creation of asset {} failed", name));
            }

            if let Some(s) = parsed["url"].take_string() {
                info!("   ... asset url = {}", s);
            }
        }

        info!("success!");
        Ok(0)
    }
}
