// Copyright 2022 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Release automation utilities related to the Zenodo service.

use anyhow::{anyhow, bail, ensure};
use chrono::prelude::*;
use json::JsonValue;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{self, Map, Value};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};
use structopt::StructOpt;

use super::Command;
use crate::{
    a_ok_or, app::AppSession, atry, env::require_var, errors::Result, project::Project, write_crlf,
};

/// A type for interacting with the Zenodo REST API.
#[derive(Debug)]
struct ZenodoService {
    token: String,
}

impl ZenodoService {
    fn new() -> Result<Self> {
        let token = require_var("ZENODO_TOKEN")?;

        Ok(ZenodoService { token })
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
            .timeout(None) // default is 30s; Zenodo is slow
            .build()?)
    }

    fn api_url(&self, rest: &str) -> String {
        format!("https://zenodo.org/api/{}", rest)
    }
}

/// A type for abstracting between "development mode" and real Zenodo
/// deposition.
#[derive(Debug)]
struct ZenodoWorkflow<'a> {
    mode: ZenodoMode,
    proj: &'a Project,
}

#[derive(Debug)]
enum ZenodoMode {
    Development,
    Release(ZenodoService),
}

impl<'a> ZenodoWorkflow<'a> {
    fn new(proj: &'a Project, dev_mode: bool, proj_is_released: bool) -> Result<Self> {
        let mode = if dev_mode {
            info!(
                "faking Zenodo workflow for project `{}` in development mode",
                &proj.user_facing_name
            );

            // Make sure that $ZENODO_TOKEN is *not* available in the environment, since
            // this command should be run during both pull-request processing (where the
            // input is untrusted) and release processing (where it is).
            if std::env::var_os("ZENODO_TOKEN").is_some() {
                error!(
                    "the environment variable ZENODO_TOKEN is set during this development-mode job"
                );
                error!("... this could be a security risk given a malicious pull request");
                error!("... if this is a CI job, fix your configuration to only provide the variable for trusted release jobs");
                bail!("refusing to proceed");
            }

            ZenodoMode::Development
        } else if !proj_is_released {
            // We should get here if we're in a monorepo and we're processing a
            // real release request, but this particular project isn't being
            // released. To make life easier for developers, we shouldn't force
            // them to skip the preregister step, but we shouldn't actually
            // preregister the DOI! We should NOT complain if $ZENODO_TOKEN is
            // set because it needs to be available for the cases where this
            // package *is* being released.
            //
            // In principle, we should rewrite using the actual DOI of the most
            // recent release here, in the same way that we'll rewrite using the
            // *version number* of the most recent release. But in order to do
            // that, we'd need to dig the DOI metadata out of the JSON file in
            // the `release` commit, and that seems like a hassle. Unlike
            // version numbers, internal dependencies shouldn't care about DOIs,
            // so I feel OK with dodging all that and just using the dev-mode
            // path.
            info!(
                "faking Zenodo workflow for project `{}`: it is not being released",
                &proj.user_facing_name
            );

            ZenodoMode::Development
        } else {
            info!(
                "starting Zenodo workflow for project `{}` in release mode",
                &proj.user_facing_name
            );

            let svc = ZenodoService::new()?;
            ZenodoMode::Release(svc)
        };

        Ok(ZenodoWorkflow { mode, proj })
    }

    fn preregister(&self, metadata_path: &PathBuf, rewrite_paths: &[PathBuf]) -> Result<()> {
        // Fill in the metadata.

        let mut md = ZenodoMetadata::load_for_prereg(metadata_path)?;

        md.metadata.insert(
            "title".to_owned(),
            Value::String(format!(
                "{} {}",
                &self.proj.user_facing_name, &self.proj.version
            )),
        );
        md.metadata.insert(
            "version".to_owned(),
            Value::String(self.proj.version.to_string()),
        );

        let utc: DateTime<Utc> = Utc::now();
        md.metadata.insert(
            "publication_date".to_owned(),
            Value::String(format!(
                "{:>04}-{:>02}-{:>02}",
                utc.year(),
                utc.month(),
                utc.day(),
            )),
        );

        // Preregister ... or not, if we're in development mode. We further have
        // two ways to do the preregistration, depending on whether we creating
        // a wholly new "concept", or adding a new version for a preexisting
        // one.

        let new_concept = match &self.mode {
            ZenodoMode::Development => {
                md.concept_doi =
                    format!("xx.xxxx/dev-build.{}.concept", &self.proj.user_facing_name);
                md.version_rec_id = format!(
                    "dev.{}.v{}",
                    &self.proj.user_facing_name, &self.proj.version
                );
                md.version_doi = format!(
                    "xx.xxxx/dev-build.{}.v{}",
                    &self.proj.user_facing_name, &self.proj.version
                );
                false
            }

            ZenodoMode::Release(ref svc) => {
                if let Some(target_version) = md.concept_rec_id.strip_prefix("new-for:") {
                    // Registering a wholly new project, not an updated version in a
                    // series. Make sure that we're not accidentally doing that.

                    if target_version != self.proj.version.to_string() {
                        error!("the Zenodo metadata file specifies that a new \"concept DOI\" should be created");
                        error!(
                            "... but for version `{}`, while this run is for version `{}`",
                            target_version, &self.proj.version
                        );
                        error!("... this suggests that you need to update `{}` to include the Zenodo record ID of the \"concept record\"", metadata_path.display());
                        error!(
                            "... so that this release will be properly linked to previous releases"
                        );
                        error!("If you really want to create a new concept DOI, update the version in the `conceptrecid: \"new-for:...\"` specification");
                        bail!("refusing to proceed");
                    }

                    self.preregister_new_concept(svc, &mut md)?;
                    true
                } else {
                    self.preregister_existing_concept(svc, &mut md)?;
                    false
                }
            }
        };

        // Get the magic numbers into the logs.

        info!(
            "DOI for {}@{}: {}",
            &self.proj.user_facing_name, &self.proj.version, &md.version_doi
        );
        info!(
            "Zenodo record-id for {}@{}: {}",
            &self.proj.user_facing_name, &self.proj.version, &md.version_rec_id
        );

        if new_concept {
            info!(
                "Zenodo record-id for {} \"concept\": {}",
                &self.proj.user_facing_name, &md.concept_rec_id
            );
            info!(
                "... you should insert this value into the `conceptrecid` field of `{}`",
                metadata_path.display()
            );
            info!("... so that subsequent releases are properly associated with this one");
        }

        // Rewrite the metadata file with the new info.

        {
            let mut f = atry!(
                File::create(metadata_path);
                ["failed to open `{}` for rewriting", metadata_path.display()]
            );
            atry!(
                serde_json::to_writer_pretty(&mut f, &md);
                ["failed to overwrite JSON file `{}`", metadata_path.display()]
            );
            atry!(
                write_crlf!(f, "");
                ["failed to overwrite JSON file `{}`", metadata_path.display()]
            );
        }

        // Rewrite any other files the user wants.

        let mut rewrites = Vec::new();
        rewrites.push((
            format!("xx.xxxx/dev-build.{}.concept", &self.proj.user_facing_name),
            md.concept_doi.clone(),
        ));
        rewrites.push((
            format!("xx.xxxx/dev-build.{}.version", &self.proj.user_facing_name),
            md.version_doi.clone(),
        ));

        for rw_path in rewrite_paths {
            atry!(
                self.rewrite_file(rw_path, &rewrites);
                ["error while attempting to rewrite `{}`", rw_path.display()]
            );
        }

        // All done!
        Ok(())
    }

    fn preregister_new_concept(&self, svc: &ZenodoService, md: &mut ZenodoMetadata) -> Result<()> {
        let client = svc.make_blocking_client()?;
        let url = svc.api_url("deposit/depositions");
        self.send_metadata_and_slurp(&client, &url, true, md)
            .map(|_info| ())
    }

    fn preregister_existing_concept(
        &self,
        svc: &ZenodoService,
        md: &mut ZenodoMetadata,
    ) -> Result<()> {
        // The first thing we need to do is find the record_id of the most
        // recent version, which we can get from the concept record.

        let client = svc.make_blocking_client()?;
        let url = svc.api_url(&format!("records/{}", &md.concept_rec_id));

        let resp = client.get(&url).send()?;
        let status = resp.status();
        let mut parsed = json::parse(&resp.text()?)?;

        ensure!(
            status.is_success(),
            "query of concept record `{}` failed: {}",
            &md.concept_rec_id,
            parsed
        );

        let last_rec_id = match parsed["id"].take() {
            JsonValue::String(s) => s,
            JsonValue::Short(s) => s.as_str().to_owned(),
            JsonValue::Number(n) => n.to_string(),
            _ => {
                error!("Zenodo response: {}", parsed);
                bail!("queried Zenodo concept record but got no `id`");
            }
        };

        info!(
            "resolved concept rec-id {} to latest release {}",
            &md.concept_rec_id, &last_rec_id
        );

        // Now we can issue a request to create a new version for the DOI. For
        // whatever reason, this request has to be associated with the most
        // recent version, not the concept. The response to this request is also
        // info about that version, not the new version.

        let url = svc.api_url(&format!(
            "deposit/depositions/{}/actions/newversion",
            &last_rec_id
        ));
        let resp = client.post(&url).send()?;
        let status = resp.status();
        let mut parsed = json::parse(&resp.text()?)?;

        ensure!(
            status.is_success(),
            "request for new version failed: {}",
            parsed
        );

        let new_rec_url = a_ok_or!(
            parsed["links"]["latest_draft"].take_string();
            ["Zenodo new version request seems to have worked, but no `links.latest_draft` available: {}", parsed]
        );
        info!("URL of new version: {}", &new_rec_url);

        // Apply the new metadata and parse the info we need.
        //
        // We have a weakness here because Zenodo propagates metadata from
        // previous versions, so if a field is supposed to be deleted in this
        // new version, I believe that it will linger.

        let mut info = self.send_metadata_and_slurp(&client, &new_rec_url, false, md)?;

        // Continuing the theme of the above: artifacts associated with previous
        // versions will linger too. We emphatically do not want that, so let's
        // delete them.

        if let JsonValue::Array(mut files) = info["files"].take() {
            for mut fileinfo in files.drain(..) {
                let name = fileinfo["filename"]
                    .take_string()
                    .unwrap_or_else(|| "(unknown name)".to_owned());

                if let Some(url) = fileinfo["links"]["self"].take_string() {
                    info!("deleting propagated artifact file `{}` ...", name);
                    let resp = client.delete(&url).send()?;
                    let status = resp.status();
                    if !status.is_success() {
                        let t = resp.text().unwrap_or_else(|_e| {
                            "(unable to parse server response body)".to_owned()
                        });
                        warn!("failed to delete {}: {}", url, t);
                    }
                } else {
                    info!("not deleting propagated artifact file `{}` because `links.self` was not specified", name);
                }
            }
        }

        Ok(())
    }

    fn send_metadata_and_slurp(
        &self,
        client: &reqwest::blocking::Client,
        url: &str,
        do_post: bool,
        md: &mut ZenodoMetadata,
    ) -> Result<JsonValue> {
        // Prep body from metadata.

        let md_body = atry!(
            serde_json::to_string(&md.metadata);
            ["failed to serialize Zenodo metadata to JSON"]
        );
        let body = format!("{{\"metadata\":{}}}", md_body);

        // Send the request.

        let req = if do_post {
            client.post(url)
        } else {
            client.put(url)
        };
        let resp = req
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()?;
        let status = resp.status();
        let mut parsed = json::parse(&resp.text()?)?;

        if status.is_success() {
            info!("preregistration completed");
        } else {
            bail!("Zenodo preregistration failed: {}", parsed);
        }

        // See if we got everything we need from the request.

        if let Some(s) = parsed["conceptrecid"].take_string() {
            md.concept_rec_id = s;
        } else {
            error!("Zenodo response: {}", parsed);
            bail!(
                "Zenodo preregistration seems to have succeeded, but response was \
                missing `conceptrecid` string field; cannot proceed"
            );
        }

        md.version_rec_id = match parsed["record_id"].take() {
            JsonValue::String(s) => s,
            JsonValue::Short(s) => s.as_str().to_owned(),
            JsonValue::Number(n) => n.to_string(),
            _ => {
                error!("Zenodo response: {}", parsed);
                bail!(
                    "Zenodo preregistration seems to have succeeded, the `record_id`
                    field had a surprising format; cannot proceed"
                );
            }
        };

        if let Some(s) = parsed["metadata"]["prereserve_doi"]["doi"].take_string() {
            md.version_doi = s;
        } else {
            error!("Zenodo response: {}", parsed);
            bail!(
                "Zenodo preregistration seems to have succeeded, but response was \
                missing `metadata.prereserve_doi.doi` string field; cannot proceed"
            );
        }

        if let Some(s) = parsed["links"]["bucket"].take_string() {
            md.bucket_link = s;
        } else {
            error!("Zenodo response: {}", parsed);
            bail!(
                "Zenodo preregistration seems to have succeeded, but response was \
                missing `links.bucket` string field; cannot proceed"
            );
        }

        // As far as I can tell, when we're preregistering in new-concept mode,
        // the concept DOI is not yet known or registered at this point. But we
        // need it now so that it can be rewritten into the source code for
        // display to users. Fortunately (?) Zenodo DOIs are currently simple
        // functions of Zenodo record IDs, even though this is absolutely not
        // something we can rely on in general. So let's be naughty:

        let mut maybe_concept_doi = parsed["conceptdoi"].take_string().unwrap_or_default();

        if maybe_concept_doi.is_empty() {
            warn!("fabricating Zenodo concept DOI for first-time registration");
            warn!("... it could be incorrect if Zenodo changes their DOI implementation");
            maybe_concept_doi = format!("10.5281/zenodo.{}", &md.concept_rec_id);
        }

        md.concept_doi = maybe_concept_doi;
        Ok(parsed)
    }

    fn rewrite_file<P: AsRef<Path>>(&self, path: P, rewrites: &[(String, String)]) -> Result<()> {
        let path = path.as_ref();
        let mut did_anything = false;

        let cur_f = atry!(
            File::open(&path);
            ["failed to open file `{}` for reading", path.display()]
        );
        let cur_reader = BufReader::new(cur_f);

        let new_af =
            atomicwrites::AtomicFile::new(&path, atomicwrites::OverwriteBehavior::AllowOverwrite);

        let r = new_af.write(|new_f| {
            for line in cur_reader.lines() {
                let mut line = atry!(
                    line;
                    ["error reading data from file `{}`", path.display()]
                );

                for (ref template, ref replacement) in rewrites {
                    // It's going to be a little inefficient to check for contains
                    // before replacing, but otherwise I don't see a convenient way
                    // to notice that a change has been made.

                    if line.contains(template) {
                        line = line.replace(template, replacement);
                        did_anything = true;
                    }
                }

                atry!(
                    write_crlf!(new_f, "{}", line);
                    ["error writing data to `{}`", new_af.path().display()]
                );
            }

            Ok(())
        });

        match r {
            Err(atomicwrites::Error::Internal(e)) => Err(e.into()),
            Err(atomicwrites::Error::User(e)) => Err(e),
            Ok(()) => {
                if !did_anything {
                    warn!(
                        "rewriter for Zenodo DOI file `{}` didn't make any modifications",
                        path.display()
                    );
                }

                Ok(())
            }
        }
    }
}

/// The `zenodo.json5` metadata file
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ZenodoMetadata {
    /// The ID of the Zenodo concept record for a package, *or* the string
    /// "new-for:$version" if we are knowingly creating a whole new concept ID.
    #[serde(rename = "conceptrecid")]
    pub concept_rec_id: String,

    /// Zenodo metadata about the deposition. We want to be as agnostic as
    /// possible about the metadata here, but we generate certain fields
    /// automatically upon release.
    pub metadata: Map<String, Value>,

    /// The DOI of the concept record. This should not be stored in version
    /// control, but will be filled in during the preregistration step.
    #[serde(rename = "conceptdoi", default)]
    pub concept_doi: String,

    /// The ID of the Zenodo version record for this deposit. This should not be
    /// stored in version control, but will be filled in during the
    /// preregistration step.
    #[serde(rename = "record_id", default)]
    pub version_rec_id: String,

    /// The DOI of the Zenodo version record for this deposit. This should not
    /// be stored in version control, but will be filled in during the
    /// preregistration step.
    #[serde(rename = "doi", default)]
    pub version_doi: String,

    /// The URL to use for uploading artifacts. Should not be stored in version
    /// control.
    #[serde(default)]
    pub bucket_link: String,
}

impl ZenodoMetadata {
    /// Read the metadata file for the preregistration phase. Certain fields
    /// should be empty.
    fn load_for_prereg<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let md = Self::load_base(path)?;
        ensure!(
            md.concept_doi.is_empty(),
            "`conceptdoi` field of `{}` must not be specified before preregistration",
            path.display()
        );
        ensure!(
            md.version_rec_id.is_empty(),
            "`record_id` field of `{}` must not be specified before preregistration",
            path.display()
        );
        ensure!(
            md.version_doi.is_empty(),
            "`doi` field of `{}` must not be specified before preregistration",
            path.display()
        );
        ensure!(
            md.bucket_link.is_empty(),
            "`bucket_link` field of `{}` must not be specified before preregistration",
            path.display()
        );
        Ok(md)
    }

    /// Read the metadata file for the deployment phase.
    fn load_for_deployment<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let md = Self::load_base(path)?;
        ensure!(
            !md.version_doi.is_empty(),
            "`doi` field of `{}` should be specified for deployment",
            path.display()
        );
        Ok(md)
    }

    fn load_base<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        // Looks like we have to read the file all at once for json5.
        let t = atry!(
            fs::read_to_string(path);
            ["failed to read file `{}` as text", path.display()]
        );

        Ok(atry!(
            json5::from_str::<ZenodoMetadata>(&t);
            ["failed to parse file `{}` as JSON5", path.display()]
        ))
    }
}

/// The `zenodo` subcommands.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub enum ZenodoCommands {
    /// Pre-register a deposition, obtaining DOIs and applying them to the source.
    Preregister(PreregisterCommand),

    /// Publish a deposition, registering the DOI(s).
    Publish(PublishCommand),

    #[structopt(name = "upload-artifacts")]
    /// Upload one or more files as artifacts associated with a Zenodo deposit.
    UploadArtifacts(UploadArtifactsCommand),
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct ZenodoCommand {
    #[structopt(subcommand)]
    command: ZenodoCommands,
}

impl Command for ZenodoCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            ZenodoCommands::Preregister(o) => o.execute(),
            ZenodoCommands::Publish(o) => o.execute(),
            ZenodoCommands::UploadArtifacts(o) => o.execute(),
        }
    }
}

/// Pre-register a deposition, obtaining DOIs and applying them to the source.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct PreregisterCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force operation even in unexpected conditions"
    )]
    force: bool,

    #[structopt(
        long = "metadata",
        help = "The path to a JSON5 file containing Zenodo deposition metadata.",
        required = true
    )]
    metadata_path: PathBuf,

    #[structopt(
        help = "The name of the project associated with this deposition.",
        required = true
    )]
    proj_name: String,

    #[structopt(
        help = "The path(s) of file(s) to rewrite with DOI data",
        required = true
    )]
    rewrite_paths: Vec<PathBuf>,
}

impl Command for PreregisterCommand {
    fn execute(self) -> Result<i32> {
        let mut sess = AppSession::initialize_default()?;

        // Set up correct versions. This will print out version assignments.

        let (dev_mode, rci) = sess.ensure_ci_rc_mode(self.force)?;
        sess.apply_versions(&rci)?;

        // Get information about the project being released and set up the workflow.

        let ident = sess
            .graph()
            .lookup_ident(&self.proj_name)
            .ok_or_else(|| anyhow!("no such project `{}`", self.proj_name))?;

        let proj = sess.graph().lookup(ident);
        let proj_is_released = rci.lookup_project(proj).is_some();
        let wf = ZenodoWorkflow::new(proj, dev_mode, proj_is_released)?;

        // Go!

        wf.preregister(&self.metadata_path, &self.rewrite_paths[..])?;
        Ok(0)
    }
}

/// Publish a deposition, registering the DOI(s).
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct PublishCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force operation even in unexpected conditions"
    )]
    force: bool,

    #[structopt(
        long = "metadata",
        help = "The path to a JSON5 file containing Zenodo deposition metadata.",
        required = true
    )]
    metadata_path: PathBuf,
}

impl Command for PublishCommand {
    fn execute(self) -> Result<i32> {
        let sess = AppSession::initialize_default()?;
        let (dev_mode, _rel_info) = sess.ensure_ci_release_mode()?;

        if dev_mode {
            if self.force {
                warn!("should not publish to Zenodo in development mode, but you're forcing me to");
            } else {
                error!("do not publish to Zenodo in development mode");
                bail!("refusing to proceed (use `--force` to override)",);
            }
        }

        let md = atry!(
            ZenodoMetadata::load_for_deployment(&self.metadata_path);
            ["failed to load Zenodo metadata file `{}`", &self.metadata_path.display()]
        );

        let svc = ZenodoService::new()?;
        let client = svc.make_blocking_client()?;

        // Tests indicate that we need to update `state` at this point. Zenodo
        // won't let us update only that field; we need to include the
        // "metadata" section as well.

        let md_body = atry!(
            serde_json::to_string(&md.metadata);
            ["failed to serialize Zenodo metadata to JSON"]
        );
        let body = format!("{{\"metadata\":{}, \"state\": \"done\"}}", md_body);

        let url = svc.api_url(&format!("deposit/depositions/{}", &md.version_rec_id));
        let resp = client
            .put(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()?;
        let status = resp.status();
        let parsed = json::parse(&resp.text()?)?;

        if !status.is_success() {
            error!("Zenodo metadata API response: {}", parsed);
            bail!("publication of record `{}` failed", &md.version_rec_id);
        }

        // Final publication:

        let url = svc.api_url(&format!(
            "deposit/depositions/{}/actions/publish",
            &md.version_rec_id
        ));
        let resp = client.post(&url).send()?;
        let status = resp.status();
        let parsed = json::parse(&resp.text()?)?;

        if !status.is_success() {
            error!("Zenodo API response: {}", parsed);
            bail!("publication of record `{}` failed", &md.version_rec_id);
        }

        info!(
            "publication successful - view at https://zenodo.org/record/{}",
            &md.version_rec_id
        );
        Ok(0)
    }
}

/// Upload one or more files as artifacts associated with a Zenodo deposit.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub struct UploadArtifactsCommand {
    #[structopt(
        short = "f",
        long = "force",
        help = "Force operation even in unexpected conditions"
    )]
    force: bool,

    #[structopt(
        long = "metadata",
        help = "The path to a JSON5 file containing Zenodo deposition metadata.",
        required = true
    )]
    metadata_path: PathBuf,

    #[structopt(help = "The path(s) to the file(s) to upload", required = true)]
    paths: Vec<PathBuf>,
}

impl Command for UploadArtifactsCommand {
    fn execute(self) -> Result<i32> {
        let sess = AppSession::initialize_default()?;
        let (dev_mode, _rel_info) = sess.ensure_ci_release_mode()?;

        if dev_mode {
            if self.force {
                warn!("should not upload artifacts in development mode, but you're forcing me to");
            } else {
                error!("do not upload artifacts in development mode");
                bail!("refusing to proceed (use `--force` to override)",);
            }
        }

        let md = atry!(
            ZenodoMetadata::load_for_deployment(&self.metadata_path);
            ["failed to load Zenodo metadata file `{}`", &self.metadata_path.display()]
        );

        let svc = ZenodoService::new()?;
        let client = svc.make_blocking_client()?;

        // Ready to go

        for path in &self.paths {
            // Make sure the file exists!
            let file = File::open(path)?;

            let name = path
                .file_name()
                .ok_or_else(|| anyhow!("input file has no name component??"))?
                .to_str()
                .ok_or_else(|| anyhow!("input file name cannot be stringified"))?
                .to_owned();

            let enc =
                percent_encoding::utf8_percent_encode(&name, percent_encoding::NON_ALPHANUMERIC);
            info!("uploading `{}` => {}", path.display(), &name);

            let url = format!("{}/{}", md.bucket_link, enc);
            let resp = client
                .put(&url)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(file)
                .send()?;
            let status = resp.status();
            let parsed = json::parse(&resp.text()?)?;

            if !status.is_success() {
                error!("Zenodo API response: {}", parsed);
                bail!("creation of asset `{}` failed", name);
            }

            // On success, we don't have anything important to do with the
            // response.
        }

        Ok(0)
    }
}
