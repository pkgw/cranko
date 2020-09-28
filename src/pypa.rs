// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Python Packaging Authority (PyPA) projects.

use anyhow::{anyhow, bail, Context};
use configparser::ini::Ini;
use log::warn;
use serde::Deserialize;
use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    process,
};
use structopt::StructOpt;
use toml::Value;

use super::Command;

use crate::{
    a_ok_or,
    app::{AppBuilder, AppSession},
    atry,
    errors::{Error, Result},
    graph::GraphQueryBuilder,
    repository::{RepoPath, RepoPathBuf},
    version::{Pep440Version, Version},
};

/// Framework for auto-loading PyPA projects from the repository contents.
#[derive(Debug, Default)]
pub struct PypaLoader {
    dirs_of_interest: HashSet<RepoPathBuf>,
}

impl PypaLoader {
    pub fn process_index_item(&mut self, dirname: &RepoPath, basename: &RepoPath) {
        let b = basename.as_ref();

        if b == b"setup.py" || b == b"setup.cfg" && b == b"pyproject.toml" {
            self.dirs_of_interest.insert(dirname.to_owned());
        }
    }

    /// Finalize autoloading any PyPA projects. Consumes this object.
    pub fn finalize(self, app: &mut AppBuilder) -> Result<()> {
        if self.dirs_of_interest.len() > 1 {
            warn!("multiple Python projects detected. Internal interdependenciess are not yet supported.")
        }

        for dirname in &self.dirs_of_interest {
            let mut name = None;
            let mut version = None;
            let mut main_version_file = None;

            let dir_desc = if dirname.len() == 0 {
                "the toplevel directory".to_owned()
            } else {
                format!("directory `{}`", dirname.escaped())
            };

            // Try pyproject.toml first. If it exists, it might contain metadata
            // that help us gather info from the other project files.

            {
                let mut toml_path = dirname.clone();
                toml_path.push("pyproject.toml");
                let toml_path = app.repo.resolve_workdir(&toml_path);

                let f = match File::open(&toml_path) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            None
                        } else {
                            return Err(Error::new(e).context(format!(
                                "failed to open file `{}`",
                                toml_path.display()
                            )));
                        }
                    }
                };

                let data = f
                    .map(|mut f| -> Result<PyProjectFile> {
                        let mut text = String::new();
                        atry!(
                            f.read_to_string(&mut text);
                            ["failed to read file `{}`", toml_path.display()]
                        );

                        Ok(atry!(
                            toml::from_str(&text);
                            ["could not parse file `{}` as TOML", toml_path.display()]
                        ))
                    })
                    .transpose()?;

                let data = data.map(|d| d.tool).flatten().map(|t| t.cranko).flatten();

                if let Some(data) = data {
                    name = data.name;
                    main_version_file = data.main_version_file;
                }
            }

            // Now let's see if we have anything to learn from `setup.cfg`.
            //
            // TODO: in some projects setup.cfg might be the place to look for
            // the version, but in most of the examples I'm checking, it isn't,
            // so we don't wire that up.

            {
                let mut cfg_path = dirname.clone();
                cfg_path.push("setup.cfg");
                let cfg_path = app.repo.resolve_workdir(&cfg_path);

                let f = match File::open(&cfg_path) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            None
                        } else {
                            return Err(Error::new(e)
                                .context(format!("failed to open file `{}`", cfg_path.display())));
                        }
                    }
                };

                let data = f
                    .map(|mut f| -> Result<Ini> {
                        let mut text = String::new();
                        atry!(
                            f.read_to_string(&mut text);
                            ["failed to read file `{}`", cfg_path.display()]
                        );

                        let mut cfg = Ini::new();
                        atry!(
                            cfg.read(text).map_err(|msg| anyhow!("{}", msg));
                            ["could not parse file `{}` as \"ini\"-style configuration", cfg_path.display()]
                        );

                        Ok(cfg)
                    })
                    .transpose()?;

                if let Some(data) = data {
                    if name.is_none() {
                        name = data.get("metadata", "name");
                    }
                }
            }

            let main_version_file = main_version_file.unwrap_or_else(|| "setup.py".to_owned());
            let main_version_in_setup = main_version_file == "setup.py";

            // Finally, how about setup.py?

            {
                let mut setup_path = dirname.clone();
                setup_path.push("setup.py");
                let setup_path = app.repo.resolve_workdir(&setup_path);

                let f = match File::open(&setup_path) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            None
                        } else {
                            return Err(Error::new(e).context(format!(
                                "failed to open file `{}`",
                                setup_path.display()
                            )));
                        }
                    }
                };

                if let Some(f) = f {
                    let reader = BufReader::new(f);

                    for line in reader.lines() {
                        let line = atry!(
                            line;
                            ["error reading data from file `{}`", setup_path.display()]
                        );

                        if simple_py_parse::has_commented_marker(&line, "cranko project-name") {
                            if name.is_none() {
                                name = Some(atry!(
                                    simple_py_parse::extract_text_from_string_literal(&line);
                                    ["failed to determine Python project name from `{}`", setup_path.display()]
                                ));
                            }
                        }

                        if main_version_in_setup
                            && simple_py_parse::has_commented_marker(
                                &line,
                                "cranko project-version",
                            )
                        {
                            version = Some(atry!(
                                version_from_line(&line);
                                ["failed to parse project version out source text line `{}` in `{}`",
                                 line, setup_path.display()]
                            ));
                        }
                    }
                }
            }

            fn version_from_line(line: &str) -> Result<Pep440Version> {
                if simple_py_parse::has_commented_marker(&line, "cranko project-version tuple") {
                    Pep440Version::parse_from_tuple_literal(&line)
                } else {
                    Ok(simple_py_parse::extract_text_from_string_literal(line)?.parse()?)
                }
            }

            // Do we need to look in yet another file to pull out the version?

            if !main_version_in_setup {
                let mut version_path = dirname.clone();
                version_path.push(main_version_file);
                let version_path = app.repo.resolve_workdir(&version_path);

                let f = atry!(
                    File::open(&version_path);
                    ["failed to open file `{}`", version_path.display()]
                );

                let reader = BufReader::new(f);

                for line in reader.lines() {
                    let line = atry!(
                        line;
                        ["error reading data from file `{}`", version_path.display()]
                    );

                    if simple_py_parse::has_commented_marker(&line, "cranko project-version") {
                        version = Some(atry!(
                            version_from_line(&line);
                            ["failed to parse project version out source text line `{}` in `{}`",
                                line, version_path.display()]
                        ));
                    }
                }
            }

            // OK, did we get everything we needed?

            let name = a_ok_or!(name;
                ["could not identify the name of the Python project in {}", dir_desc]
                (note "try adding (1) a `name = ...` field in the `[metadata]` section of its `setup.cfg` \
                      or (2) a `# cranko project-name` comment at the end of a line containing the project \
                      name as a simple string literal in `setup.py` or (3) or a `name = ...` field in a \
                      `[tool.cranko]` section of its `pyproject.toml`")
            );

            let version = a_ok_or!(version;
                ["could not identify the version of the Python project in {}", dir_desc]
                (note "try adding a `# cranko project-version` comment at the end of a line containing \
                      the project version as a simple string literal in `setup.py`; see the documentation \
                      for other supported approaches")
            );

            // OMG, we actually have everything that we need.

            let ident = app.graph.add_project();
            let mut proj = app.graph.lookup_mut(ident);

            proj.qnames = vec![name, "pypa".to_owned()];
            proj.version = Some(Version::Pep440(version));
            proj.prefix = Some(dirname.to_owned());

            // let cargo_rewrite = CargoRewriter::new(ident, manifest_repopath);
            // proj.rewriters.push(Box::new(cargo_rewrite));
        }

        Ok(())
    }
}

mod simple_py_parse {
    use super::*;

    pub fn has_commented_marker(line: &str, marker: &str) -> bool {
        match line.find('#') {
            None => false,

            Some(cidx) => match line.find(marker) {
                None => false,
                Some(midx) => midx > cidx,
            },
        }
    }

    pub fn extract_text_from_string_literal(line: &str) -> Result<String> {
        let mut sq_loc = line.find('\'');
        let mut dq_loc = line.find('"');

        // if both kinds of quotes, go with whichever we saw first.
        if let (Some(sq_idx), Some(dq_idx)) = (sq_loc, dq_loc) {
            if sq_idx < dq_idx {
                dq_loc = None;
            } else {
                sq_loc = None;
            }
        }

        let inside = if let Some(sq_left) = sq_loc {
            let sq_right = line.rfind('\'').unwrap();
            if sq_right <= sq_left {
                bail!(
                    "expected a string literal in Python line `{}`, but only found one quote?",
                    line
                );
            }

            &line[sq_left + 1..sq_right]
        } else if let Some(dq_left) = dq_loc {
            let dq_right = line.rfind('"').unwrap();
            if dq_right <= dq_left {
                bail!(
                    "expected a string literal in Python line `{}`, but only found one quote?",
                    line
                );
            }

            &line[dq_left + 1..dq_right]
        } else {
            bail!(
                "expected a string literal in Python line `{}`, but didn't find any quotation marks",
                line
            );
        };

        if inside.find('\\').is_some() {
            bail!("the string literal in Python line `{}` seems to contain \\ escapes, which I can't handle", line);
        }

        Ok(inside.to_owned())
    }
}

/// Toplevel `pyproject.toml` deserialization container.
#[derive(Debug, Deserialize)]
struct PyProjectFile {
    pub tool: Option<PyProjectTool>,

    #[serde(flatten)]
    pub rest: Value,
}

/// `pyproject.toml` section `tool` deserialization container.
#[derive(Debug, Deserialize)]
struct PyProjectTool {
    pub cranko: Option<PyProjectCranko>,

    #[serde(flatten)]
    pub rest: Value,
}

/// Cranko metadata in `pyproject.toml`.
#[derive(Debug, Deserialize)]
struct PyProjectCranko {
    /// The project name. It isn't always straightforward to determine this,
    /// since we basically can't assume anything about setup.py.
    pub name: Option<String>,

    /// The file that we should read to discover the current project version.
    /// Note that there might be other files that also contain the version that
    /// will need to be rewritten when we apply a new version.
    pub main_version_file: Option<String>,
}

/// Python-specific CLI utilities.
#[derive(Debug, PartialEq, StructOpt)]
pub enum PythonCommands {
    #[structopt(name = "foreach-released")]
    /// Run a command for each released PyPA project.
    ForeachReleased(ForeachReleasedCommand),

    #[structopt(name = "install-token")]
    /// Install $PYPI_TOKEN in the user's .pypirc.
    InstallToken(InstallTokenCommand),
}

#[derive(Debug, PartialEq, StructOpt)]
pub struct PythonCommand {
    #[structopt(subcommand)]
    command: PythonCommands,
}

impl Command for PythonCommand {
    fn execute(self) -> Result<i32> {
        match self.command {
            PythonCommands::ForeachReleased(o) => o.execute(),
            PythonCommands::InstallToken(o) => o.execute(),
        }
    }
}

/// `cranko python foreach-released`
#[derive(Debug, PartialEq, StructOpt)]
pub struct ForeachReleasedCommand {
    #[structopt(help = "The command to run", required = true)]
    command: Vec<OsString>,
}

impl Command for ForeachReleasedCommand {
    fn execute(self) -> Result<i32> {
        let sess = AppSession::initialize_default()?;

        let (dev_mode, rel_info) = sess.ensure_ci_release_mode()?;
        if dev_mode {
            warn!("proceeding even though in dev mode");
        }

        let mut q = GraphQueryBuilder::default();
        q.only_new_releases(rel_info);
        q.only_project_type("pypa");
        let idents = sess
            .graph()
            .query(q)
            .context("could not select projects for `python foreach-released`")?;

        let mut cmd = process::Command::new(&self.command[0]);
        if self.command.len() > 1 {
            cmd.args(&self.command[1..]);
        }

        let print_which = idents.len() > 1;
        let mut first = true;

        for ident in &idents {
            let proj = sess.graph().lookup(*ident);
            let dir = sess.repo.resolve_workdir(&proj.prefix());
            cmd.current_dir(&dir);

            if print_which {
                if first {
                    first = false;
                } else {
                    println!();
                }
                println!("### in `{}`:", dir.display());
            }

            let status = cmd.status().context(format!(
                "could not run the command for PyPA project `{}`",
                proj.user_facing_name
            ))?;
            if !status.success() {
                return Err(anyhow!(
                    "the command failed for PyPA project `{}`",
                    proj.user_facing_name
                ));
            }
        }

        Ok(0)
    }
}

/// `cranko python install-token`
#[derive(Debug, PartialEq, StructOpt)]
pub struct InstallTokenCommand {
    #[structopt(
        long = "repository",
        default_value = "pypi",
        help = "The repository name."
    )]
    repository: String,
}

impl Command for InstallTokenCommand {
    fn execute(self) -> Result<i32> {
        let token = atry!(
            env::var("PYPI_TOKEN");
            ["missing or non-textual environment variable PYPI_TOKEN"]
        );

        let mut p =
            dirs::home_dir().ok_or_else(|| anyhow!("cannot determine user's home directory"))?;
        p.push(".pypirc");

        let mut file = atry!(
            OpenOptions::new().write(true).create(true).append(true).open(&p);
            ["failed to open file `{}` for appending", p.display()]
        );

        let mut write = || -> Result<()> {
            writeln!(file, "[{}]", self.repository)?;
            writeln!(file, "username = __token__")?;
            writeln!(file, "password = {}", token)?;
            Ok(())
        };

        atry!(
            write();
            ["failed to write token data to file `{}`", p.display()]
        );

        Ok(0)
    }
}
