// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Python Packaging Authority (PyPA) projects.

use anyhow::{anyhow, bail, Context};
use configparser::ini::Ini;
use log::warn;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read},
    process,
};
use structopt::StructOpt;
use toml::Value;

use super::Command;

use crate::{
    a_ok_or,
    app::{AppBuilder, AppSession},
    atry,
    config::ProjectConfiguration,
    errors::{Error, Result},
    graph::GraphQueryBuilder,
    project::{DepRequirement, DependencyTarget, ProjectId},
    repository::{ChangeList, RepoPath, RepoPathBuf},
    rewriters::Rewriter,
    version::{Pep440Version, Version},
    write_crlf,
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
    pub fn finalize(
        self,
        app: &mut AppBuilder,
        pconfig: &HashMap<String, ProjectConfiguration>,
    ) -> Result<()> {
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

            let mut toml_repopath = dirname.clone();
            toml_repopath.push("pyproject.toml");

            let config = {
                let toml_path = app.repo.resolve_workdir(&toml_repopath);
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

                let data = data.and_then(|d| d.tool).and_then(|t| t.cranko);

                if let Some(ref data) = data {
                    name = data.name.clone();
                    main_version_file = data.main_version_file.clone();
                }

                data
            };

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

                        if simple_py_parse::has_commented_marker(&line, "cranko project-name")
                            && name.is_none()
                        {
                            name = Some(atry!(
                                simple_py_parse::extract_text_from_string_literal(&line);
                                ["failed to determine Python project name from `{}`", setup_path.display()]
                            ));
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
                if simple_py_parse::has_commented_marker(line, "cranko project-version tuple") {
                    Pep440Version::parse_from_tuple_literal(line)
                } else {
                    Ok(simple_py_parse::extract_text_from_string_literal(line)?.parse()?)
                }
            }

            // Do we need to look in yet another file to pull out the version?

            if !main_version_in_setup {
                let mut version_path = dirname.clone();
                version_path.push(&main_version_file);
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

            // OK, did we get the core information?

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

            // OMG, we actually have the core info.

            let qnames = vec![name.clone(), "pypa".to_owned()];

            if let Some(ident) = app.graph.try_add_project(qnames, pconfig) {
                {
                    let proj = app.graph.lookup_mut(ident);

                    proj.version = Some(Version::Pep440(version));
                    proj.prefix = Some(dirname.to_owned());

                    let mut rw_path = dirname.clone();
                    rw_path.push(main_version_file.as_bytes());
                    let rw = PythonRewriter::new(ident, rw_path);
                    proj.rewriters.push(Box::new(rw));
                }

                // Handle the other annotated files. Besides registering them for
                // rewrites, we also scan them now to detect additional metadata. In
                // particular, dependencies on non-Python projects.

                let mut internal_reqs = HashSet::new();

                for path in config
                    .as_ref()
                    .map(|c| &c.annotated_files[..])
                    .unwrap_or(&[])
                {
                    let mut rw_path = dirname.clone();
                    rw_path.push(path.as_bytes());

                    atry!(
                        scan_rewritten_file(app, &rw_path, &mut internal_reqs);
                        ["in Python project {}, could not scan the `annotated_files` entry {}",
                        dir_desc, rw_path.escaped()]
                    );

                    let rw = PythonRewriter::new(ident, rw_path);
                    {
                        let proj = app.graph.lookup_mut(ident);
                        proj.rewriters.push(Box::new(rw));
                    }
                }

                // Now that we have *all* of the internal requirements, register them with
                // the graph.

                for req_name in &internal_reqs {
                    let req = config
                        .as_ref()
                        .and_then(|c| c.internal_dep_versions.get(req_name))
                        .map(|text| app.repo.parse_history_ref(text))
                        .transpose()?
                        .map(|cref| app.repo.resolve_history_ref(&cref, &toml_repopath))
                        .transpose()?;

                    if req.is_none() {
                        warn!(
                            "missing or invalid key `tool.cranko.internal_dep_versions.{}` in `{}`",
                            &req_name,
                            toml_repopath.escaped()
                        );
                        warn!("... this is needed to specify the oldest version of `{}` compatible with `{}`",
                            &req_name, &name);
                    }

                    let req = req.unwrap_or(DepRequirement::Unavailable);
                    app.graph.add_dependency(
                        ident,
                        DependencyTarget::Text(req_name.clone()),
                        "(unavailable)".to_owned(),
                        req,
                    )
                }
            }
        }

        Ok(())
    }
}

fn scan_rewritten_file(
    app: &mut AppBuilder,
    path: &RepoPath,
    reqs: &mut HashSet<String>,
) -> Result<()> {
    let file_path = app.repo.resolve_workdir(path);

    let f = atry!(
        File::open(&file_path);
        ["failed to open file `{}` for reading", file_path.display()]
    );
    let reader = BufReader::new(f);

    for (line_num0, line) in reader.lines().enumerate() {
        let line = atry!(
            line;
            ["error reading data from file `{}`", file_path.display()]
        );

        if simple_py_parse::has_commented_marker(&line, "cranko internal-req") {
            let idx = line.find("cranko internal-req").unwrap();
            let mut pieces = line[idx..].split_whitespace();
            pieces.next(); // skip "cranko"
            pieces.next(); // skip "internal-req"
            let name = a_ok_or!(
                pieces.next();
                ["in `{}` line {}, `cranko internal-req` comment must provide a project name",
                 file_path.display(), line_num0 + 1]
            );

            reqs.insert(name.to_owned());
        }
    }

    Ok(())
}

pub(crate) mod simple_py_parse {
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

    pub fn replace_text_in_string_literal(line: &str, new_val: &str) -> Result<String> {
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

        let (left_idx, right_idx) = if let Some(sq_left) = sq_loc {
            let sq_right = line.rfind('\'').unwrap();
            if sq_right <= sq_left {
                bail!(
                    "expected a string literal in Python line `{}`, but only found one quote?",
                    line
                );
            }

            (sq_left, sq_right)
        } else if let Some(dq_left) = dq_loc {
            let dq_right = line.rfind('"').unwrap();
            if dq_right <= dq_left {
                bail!(
                    "expected a string literal in Python line `{}`, but only found one quote?",
                    line
                );
            }

            (dq_left, dq_right)
        } else {
            bail!(
                "expected a string literal in Python line `{}`, but didn't find any quotation marks",
                line
            );
        };

        let mut replaced = line[..left_idx + 1].to_owned();
        replaced.push_str(new_val);
        replaced.push_str(&line[right_idx..]);
        Ok(replaced)
    }

    pub fn replace_tuple_literal(line: &str, new_val: &str) -> Result<String> {
        let left_idx = a_ok_or!(
            line.find('(');
            ["expected a tuple literal in Python line `{}`, but no left parenthesis", line]
        );

        let right_idx = a_ok_or!(
            line.rfind(')');
            ["expected a tuple literal in Python line `{}`, but no right parenthesis", line]
        );

        if right_idx <= left_idx {
            bail!(
                "expected a tuple literal in Python line `{}`, but parentheses don't line up",
                line
            );
        }

        let mut replaced = line[..left_idx].to_owned();
        replaced.push_str(new_val);
        replaced.push_str(&line[right_idx + 1..]);
        Ok(replaced)
    }
}

/// Toplevel `pyproject.toml` deserialization container.
#[derive(Debug, Deserialize)]
struct PyProjectFile {
    pub tool: Option<PyProjectTool>,

    #[allow(dead_code)]
    #[serde(flatten)]
    pub rest: Value,
}

/// `pyproject.toml` section `tool` deserialization container.
#[derive(Debug, Deserialize)]
struct PyProjectTool {
    pub cranko: Option<PyProjectCranko>,

    #[allow(dead_code)]
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

    /// Additional Python files that should be rewritten on metadata changes.
    #[serde(default)]
    pub annotated_files: Vec<String>,

    /// Version requirements for internal dependencies.
    #[serde(default)]
    pub internal_dep_versions: HashMap<String, String>,
}

/// Rewrite a Python file to include real version numbers.
#[derive(Debug)]
pub struct PythonRewriter {
    proj_id: ProjectId,
    file_path: RepoPathBuf,
}

impl PythonRewriter {
    /// Create a new Python file rewriter.
    pub fn new(proj_id: ProjectId, file_path: RepoPathBuf) -> Self {
        PythonRewriter { proj_id, file_path }
    }
}

impl Rewriter for PythonRewriter {
    fn rewrite(&self, app: &AppSession, changes: &mut ChangeList) -> Result<()> {
        let mut did_anything = false;
        let file_path = app.repo.resolve_workdir(&self.file_path);

        let cur_f = atry!(
            File::open(&file_path);
            ["failed to open file `{}` for reading", file_path.display()]
        );
        let cur_reader = BufReader::new(cur_f);

        // Helper table for applying internal deps if needed.

        let proj = app.graph().lookup(self.proj_id);
        let mut internal_reqs = HashMap::new();

        for dep in &proj.internal_deps[..] {
            let req_text = match dep.cranko_requirement {
                DepRequirement::Manual(ref t) => t.clone(),

                DepRequirement::Commit(_) => {
                    if let Some(ref v) = dep.resolved_version {
                        format!("^{v}")
                    } else {
                        continue;
                    }
                }

                DepRequirement::Unavailable => continue,
            };

            internal_reqs.insert(
                app.graph().lookup(dep.ident).user_facing_name.clone(),
                req_text,
            );
        }

        // OK, now rewrite the file.

        let new_af = atomicwrites::AtomicFile::new(
            &file_path,
            atomicwrites::OverwriteBehavior::AllowOverwrite,
        );

        let proj = app.graph().lookup(self.proj_id);

        let r = new_af.write(|new_f| {

            for (line_num0, line) in cur_reader.lines().enumerate() {
                let line = atry!(
                    line;
                    ["error reading data from file `{}`", file_path.display()]
                );

                let line = if simple_py_parse::has_commented_marker(&line, "cranko project-version")
                {
                    did_anything = true;

                    if simple_py_parse::has_commented_marker(&line, "cranko project-version tuple") {
                        let new_text = atry!(
                            proj.version.as_pep440_tuple_literal();
                            ["couldn't convert the project version to a `sys.version_info` tuple"]
                        );
                        atry!(
                            simple_py_parse::replace_tuple_literal(&line, &new_text);
                            ["couldn't rewrite version-tuple source line `{}`", line]
                        )
                    } else {
                        atry!(
                            simple_py_parse::replace_text_in_string_literal(&line, &proj.version.to_string());
                            ["couldn't rewrite version-string source line `{}`", line]
                        )
                    }
                } else if  simple_py_parse::has_commented_marker(&line, "cranko internal-req") {
                    did_anything = true;

                    let idx = line.find("cranko internal-req").unwrap();
                    let mut pieces = line[idx..].split_whitespace();
                    pieces.next(); // skip "cranko"
                    pieces.next(); // skip "internal-req"
                    let name = a_ok_or!(
                        pieces.next();
                        ["in `{}` line {}, `cranko internal-req` comment must provide a project name",
                        file_path.display(), line_num0 + 1]
                    );

                    // This "shouldn't happen", but could if someone edits a
                    // file between the time that the app session starts and
                    // when we get to rewriting it. That indicates something
                    // racey happening so make it a hard error.
                    let req_text = a_ok_or!(
                        internal_reqs.get(name);
                        ["found internal requirement of `{}` not traced by Cranko", name]
                    );

                    atry!(
                        simple_py_parse::replace_text_in_string_literal(&line, req_text);
                        ["couldn't rewrite internal-req source line `{}`", line]
                    )
                } else {
                    line
                };

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
                        "rewriter for Python file `{}` didn't make any modifications",
                        file_path.display()
                    );
                }

                changes.add_path(&self.file_path);
                Ok(())
            }
        }
    }
}

/// Python-specific CLI utilities.
#[derive(Debug, Eq, PartialEq, StructOpt)]
pub enum PythonCommands {
    #[structopt(name = "foreach-released")]
    /// Run a command for each released PyPA project.
    ForeachReleased(ForeachReleasedCommand),

    #[structopt(name = "install-token")]
    /// Install $PYPI_TOKEN in the user's .pypirc.
    InstallToken(InstallTokenCommand),
}

#[derive(Debug, Eq, PartialEq, StructOpt)]
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
#[derive(Debug, Eq, PartialEq, StructOpt)]
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
            let dir = sess.repo.resolve_workdir(proj.prefix());
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
#[derive(Debug, Eq, PartialEq, StructOpt)]
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
            OpenOptions::new().create(true).append(true).open(&p);
            ["failed to open file `{}` for appending", p.display()]
        );

        let mut write = || -> Result<()> {
            write_crlf!(file, "[{}]", self.repository)?;
            write_crlf!(file, "username = __token__")?;
            write_crlf!(file, "password = {}", token)?;
            Ok(())
        };

        atry!(
            write();
            ["failed to write token data to file `{}`", p.display()]
        );

        Ok(0)
    }
}
