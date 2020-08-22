// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Release automation utilities related to the GitHub service.

use anyhow::{anyhow, Context};
use json::{object, JsonValue};
use log::{error, info, warn};
use std::{env, fs::File, path::PathBuf};
use structopt::StructOpt;

use super::Command;
use crate::{
    app::AppSession,
    errors::{Error, Result},
    graph,
    project::Project,
    repository::{CommitId, ReleasedProjectInfo},
};

fn maybe_var(key: &str) -> Result<Option<String>> {
    if let Some(os_str) = env::var_os(key) {
        if let Ok(s) = os_str.into_string() {
            if s.len() > 0 {
                Ok(Some(s))
            } else {
                Ok(None)
            }
        } else {
            Err(Error::Environment(format!(
                "could not parse environment variable {} as Unicode",
                key
            )))
        }
    } else {
        Ok(None)
    }
}

fn require_var(key: &str) -> Result<String> {
    maybe_var(key)?
        .ok_or_else(|| Error::Environment(format!("environment variable {} must be provided", key)))
}

struct GitHubInformation {
    slug: String,
    token: String,
}

impl GitHubInformation {
    fn new(sess: &AppSession) -> Result<Self> {
        let token = require_var("GITHUB_TOKEN")?;

        let upstream_url = sess.repo.upstream_url()?;
        info!("upstream url: {}", upstream_url);

        let upstream_url = git_url_parse::GitUrl::parse(&upstream_url).map_err(|e| {
            Error::Environment(format!(
                "cannot parse upstream Git URL `{}`: {}",
                upstream_url, e
            ))
        })?;

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

    /// Get information about an existing release.
    fn get_release_metadata(
        &self,
        sess: &AppSession,
        proj: &Project,
        rel: &ReleasedProjectInfo,
        client: &mut reqwest::blocking::Client,
    ) -> Result<JsonValue> {
        let tag_name = sess.repo.get_tag_name(proj, rel)?;
        let query_url = self.api_url(&format!("releases/tags/{}", tag_name));

        let resp = client.get(&query_url).send()?;
        if resp.status().is_success() {
            Ok(json::parse(&resp.text()?)?)
        } else {
            Err(Error::Environment(format!(
                "no GitHub release for tag `{}`: {}",
                tag_name,
                resp.text()
                    .unwrap_or_else(|_| "[non-textual server response]".to_owned())
            )))
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

        let changelog = proj.changelog.scan_changelog(proj, &sess.repo, cid)?;

        let release_info = object! {
            "tag_name" => tag_name.clone(),
            "name" => format!("{} {}", proj.user_facing_name, proj.version),
            "body" => changelog,
            "draft" => false,
            "prerelease" => false,
        };

        let create_url = self.api_url("releases");
        let resp = client
            .post(&create_url)
            .body(json::stringify(release_info))
            .send()?;
        let status = resp.status();
        let parsed = json::parse(&resp.text()?)?;

        if status.is_success() {
            info!("created GitHub release for {}", tag_name);
            Ok(parsed)
        } else {
            Err(Error::Environment(format!(
                "failed to create GitHub release for {}: {}",
                tag_name, parsed
            )))
        }
    }
}

/// Create a new release on GitHub.
#[derive(Debug, PartialEq, StructOpt)]
pub struct CreateReleaseCommand {
    #[structopt(help = "Name(s) of the project(s) to release on GitHub")]
    proj_names: Vec<String>,
}

impl Command for CreateReleaseCommand {
    fn execute(self) -> anyhow::Result<i32> {
        let mut sess = AppSession::initialize()?;
        let info = GitHubInformation::new(&sess)?;

        sess.populated_graph()?;

        let rel_info = sess
            .repo
            .parse_release_info_from_head()
            .context("expected Cranko release metadata in the HEAD commit but could not load it")?;
        let rel_commit = rel_info
            .commit
            .as_ref()
            .ok_or_else(|| anyhow!("no commit ID for HEAD (?)"))?;

        // Get the list of projects that we're interested in.
        let mut q = graph::GraphQueryBuilder::default();
        q.names(self.proj_names);
        let empty_query = q.is_empty();
        let idents = sess
            .graph()
            .query_or_all(q)
            .context("could not select projects for GitHub release")?;

        if idents.len() == 0 {
            info!("no projects selected");
            return Ok(0);
        }

        let mut client = info.make_blocking_client()?;
        let mut n_released = 0;

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);

            if let Some(rel) = rel_info.lookup_if_released(proj) {
                info.create_release(&sess, proj, &rel, rel_commit, &mut client)?;
                n_released += 1;
            } else if !empty_query {
                warn!(
                    "project {} was specified but does not have a new release",
                    proj.user_facing_name
                );
            }
        }

        if empty_query && n_released != 1 {
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

/// Upload an artifact file to a GitHub release.
#[derive(Debug, PartialEq, StructOpt)]
pub struct UploadArtifactCommand {
    #[structopt(
        long = "overwrite",
        help = "Overwrite artifacts if they already exist in the release (default: error out)"
    )]
    overwrite: bool,

    #[structopt(help = "The released project for which to upload content")]
    proj_name: String,

    #[structopt(help = "The path(s) to the file(s) to upload", required = true)]
    paths: Vec<PathBuf>,
}

impl Command for UploadArtifactCommand {
    fn execute(self) -> anyhow::Result<i32> {
        let mut sess = AppSession::initialize()?;
        let info = GitHubInformation::new(&sess)?;

        sess.populated_graph()?;

        let rel_info = sess
            .repo
            .parse_release_info_from_head()
            .context("expected Cranko release metadata in the HEAD commit but could not load it")?;

        let mut client = info.make_blocking_client()?;

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
        let mut metadata = info.get_release_metadata(&sess, proj, rel, &mut client)?;
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

#[derive(Debug, PartialEq, StructOpt)]
pub enum GithubCommands {
    #[structopt(name = "create-release")]
    /// Create one or more new GitHub releases
    CreateRelease(CreateReleaseCommand),

    #[structopt(name = "upload-artifact")]
    /// Upload a file as a GitHub release artifact
    UploadArtifact(UploadArtifactCommand),
}

#[derive(Debug, PartialEq, StructOpt)]
pub struct GithubCommand {
    #[structopt(subcommand)]
    command: GithubCommands,
}

impl Command for GithubCommand {
    fn execute(self) -> anyhow::Result<i32> {
        match self.command {
            GithubCommands::CreateRelease(o) => o.execute(),
            GithubCommands::UploadArtifact(o) => o.execute(),
        }
    }
}
