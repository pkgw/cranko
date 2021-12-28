// Copyright 2021 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Visual Studio C# projects.
//!
//! We currently "manually" update `Properties/AssemblyInfo.cs`.

use anyhow::bail;
use log::warn;
use quick_xml::{events::Event, Reader};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Write},
};

use crate::{
    a_ok_or,
    app::{AppBuilder, AppSession},
    atry,
    errors::Result,
    project::{DepRequirement, DependencyTarget, ProjectId},
    repository::{ChangeList, RepoPath, RepoPathBuf, Repository},
    rewriters::Rewriter,
    version::Version,
};

/// Framework for auto-loading Visual Studio C# projects from the repository
/// contents.
#[derive(Debug, Default)]
pub struct CsProjLoader {
    dirs_of_interest: HashMap<RepoPathBuf, DirData>,
}

#[derive(Debug, Default)]
struct DirData {
    csproj: Option<RepoPathBuf>,
    assembly_info: Option<RepoPathBuf>,
}

impl CsProjLoader {
    pub fn process_index_item(
        &mut self,
        _repo: &Repository,
        repopath: &RepoPath,
        dirname: &RepoPath,
        basename: &RepoPath,
    ) -> Result<()> {
        if basename.ends_with(b".csproj") {
            let dir = dirname.to_owned();
            let mut e = self.dirs_of_interest.entry(dir).or_default();
            e.csproj = Some(repopath.to_owned());
        } else if basename.as_ref() == b"AssemblyInfo.cs" {
            // Hardcode the assumption that we should walk up one directory:
            let (dir, _base) = dirname.pop_sep().split_basename();
            let dir = dir.to_owned();
            let mut e = self.dirs_of_interest.entry(dir).or_default();
            e.assembly_info = Some(repopath.to_owned());
        }

        Ok(())
    }

    /// Finalize autoloading any CsProj projects. Consumes this object.
    pub fn finalize(self, app: &mut AppBuilder) -> Result<()> {
        // Build up the table of projects and their deps.

        struct Info {
            ident: ProjectId,
            name: String,

            /// (guid, literal-req, resolved-req)
            deps: Vec<(String, String, DepRequirement)>,
        }

        let mut guid_to_info = HashMap::new();
        let mut gave_dep_warning_help = false;

        for (repodir, data) in &self.dirs_of_interest {
            // Basic checking that we got both a csproj and an assemblyinfo.

            let csproj = match data.csproj {
                Some(ref d) => d,
                None => {
                    // This warning could get super annoying, but in my first use case
                    // it shouldn't happen.
                    warn!(
                        "ignoring directory `{}` that has an AssemblyInfo.cs but no .csproj file",
                        repodir.escaped()
                    );
                    continue;
                }
            };

            let assembly_info = match data.assembly_info {
                Some(ref d) => d,
                None => {
                    warn!(
                        "ignoring directory `{}` that has a .csproj file but no Properties/AssemblyInfo.cs",
                        repodir.escaped()
                    );
                    continue;
                }
            };

            // Parse the .csproj XML

            let p = app.repo.resolve_workdir(csproj);
            let mut xml = atry!(
                Reader::from_file(&p);
                ["unable to open `{}` for reading", p.display()]
            );
            let mut buf = Vec::new();
            let mut guid = None;
            let mut name = None;
            let mut dep_guids = Vec::new();
            let mut dep_reqs = HashMap::new();
            let mut state = State::Scanning;

            enum State {
                Scanning,
                GuidText,
                NameText,
                DepGuidText,
                DepReqText,
            }

            loop {
                match xml.read_event(&mut buf) {
                    Ok(Event::Start(ref e)) => {
                        state = match e.name() {
                            b"ProjectGuid" => State::GuidText,
                            b"AssemblyName" => State::NameText,
                            b"Project" => {
                                // Make sure to ignore the toplevel <Project> element!
                                if guid.is_some() {
                                    State::DepGuidText
                                } else {
                                    State::Scanning
                                }
                            }
                            b"CrankoInternalDepVersion" => State::DepReqText,
                            _ => State::Scanning,
                        };
                    }

                    Ok(Event::Text(ref t)) => match state {
                        State::GuidText => {
                            let mut g = atry!(
                                t.unescape_and_decode_without_bom(&xml);
                                ["unable to decode XML text in ProjectGuid of `{}`", p.display()]
                            );
                            g.make_ascii_lowercase();
                            guid = Some(g);
                            state = State::Scanning;
                        }

                        State::NameText => {
                            name = Some(atry!(
                                t.unescape_and_decode_without_bom(&xml);
                                ["unable to decode XML text in AssemblyName of `{}`", p.display()]
                            ));
                            state = State::Scanning;
                        }

                        State::DepGuidText => {
                            let mut g = atry!(
                                t.unescape_and_decode_without_bom(&xml);
                                ["unable to decode XML text in <Project> of `{}`", p.display()]
                            );
                            g.make_ascii_lowercase();
                            dep_guids.push(g);
                            state = State::Scanning;
                        }

                        State::DepReqText => {
                            let r = atry!(
                                t.unescape_and_decode_without_bom(&xml);
                                ["unable to decode XML text in CrankoInternalDepVersion of `{}`", p.display()]
                            );
                            let (guid, reqtext) = a_ok_or!(
                                r.split_once('=');
                                ["malformatted CrankoInternalDepVersion `{}` in `{}`", r, p.display()]
                            );
                            let mut guid = guid.to_owned();
                            guid.make_ascii_lowercase();
                            dep_reqs.insert(guid, reqtext.to_owned());
                            state = State::Scanning;
                        }

                        State::Scanning => {}
                    },

                    Ok(Event::End(_)) => state = State::Scanning,

                    Ok(Event::Eof) => break,

                    Err(e) => {
                        return Err(anyhow::Error::new(e)
                            .context(format!("error parsing `{}` as XML", p.display())))
                    }

                    _ => {}
                }
            }

            let guid = match guid {
                Some(g) => g,
                None => {
                    warn!(
                        "ignoring .csproj file `{}`: cannot find its ProjectGuid",
                        p.display()
                    );
                    continue;
                }
            };

            let name = match name {
                Some(n) => n,
                None => {
                    warn!(
                        "ignoring .csproj file `{}`: cannot find its ProjectGuid",
                        p.display()
                    );
                    continue;
                }
            };

            // Process the internal dep requirements.

            let mut resolved_reqs = Vec::new();

            for guid in dep_guids.drain(..) {
                let (req, text) = if let Some(text) = dep_reqs.get(&guid) {
                    match app
                        .repo
                        .parse_history_ref(text)
                        .and_then(|cref| app.repo.resolve_history_ref(&cref, csproj))
                    {
                        Ok(r) => (r, text.clone()),

                        Err(e) => {
                            warn!(
                                "invalid <CrankoInternalDepVersion> entry `{}` in `{}`: {}",
                                text,
                                p.display(),
                                e
                            );
                            (DepRequirement::Unavailable, text.clone())
                        }
                    }
                } else {
                    (DepRequirement::Unavailable, "UNDEFINED".to_owned())
                };

                if req == DepRequirement::Unavailable {
                    warn!("cannot find version requirement for dependency of `{}` on the project with GUID `{}`", &name, &guid);

                    if !gave_dep_warning_help {
                        warn!("... you likely need to add <CrankoInternalDepVersion>{{guid}}={{req}}</CrankoInternalDepVersion> to `{}`", p.display());
                        warn!("... Cranko needs this information to know the oldest compatible version of the dependency");
                        gave_dep_warning_help = true;
                    }
                }

                resolved_reqs.push((guid, text, req));
            }

            // Now parse the assembly info ...

            let mut version = None;
            let p = app.repo.resolve_workdir(assembly_info);

            {
                let f = atry!(
                    File::open(&p);
                    ["failed to open file `{}`", p.display()]
                );
                let reader = BufReader::new(f);

                for line in reader.lines() {
                    let line = atry!(
                        line;
                        ["error reading data from file `{}`", p.display()]
                    );

                    if line.starts_with("[assembly: AssemblyVersion") {
                        let l1 = a_ok_or!(
                            line.find('"');
                            ["error parsing AssemblyVersion line in file `{}`", p.display()]
                        );
                        let l2 = a_ok_or!(
                            line.rfind('"');
                            ["error parsing AssemblyVersion line in file `{}`", p.display()]
                        );
                        let v = atry!(
                            line[l1 + 1..l2].parse();
                            ["error parsing AssemblyVersion line in file `{}`", p.display()]
                        );
                        version = Some(Version::DotNet(v));
                    }
                }
            }

            let version = match version {
                Some(v) => v,
                None => {
                    warn!(
                        "ignoring project in `{}`: cannot find its version in `{}`",
                        repodir.escaped(),
                        p.display()
                    );
                    continue;
                }
            };

            // Finally we can register this project.

            let ident = app.graph.add_project();
            let mut proj = app.graph.lookup_mut(ident);
            proj.qnames = vec![name.to_owned(), "csproj".to_owned()];
            proj.prefix = Some(repodir.to_owned());
            proj.version = Some(version);

            // Auto-register a rewriter to update this package's `AssemblyInfo.cs`.
            let rewrite = AssemblyInfoCsRewriter::new(ident, assembly_info.to_owned());
            proj.rewriters.push(Box::new(rewrite));

            // Save the info for dep-linking.

            guid_to_info.insert(
                guid,
                Info {
                    ident,
                    name: name.to_owned(),
                    deps: resolved_reqs,
                },
            );
        }

        // Now that we've registered them all, we can populate the interdependencies.

        for info in guid_to_info.values() {
            for (guid, literal, req) in &info.deps {
                let dep_ident = match guid_to_info.get(guid) {
                    Some(dep_info) => dep_info.ident,
                    None => bail!(
                        "C# project `{}` depends on a project with GUID {} but I cannot locate it",
                        info.name,
                        guid
                    ),
                };

                app.graph.add_dependency(
                    info.ident,
                    DependencyTarget::Ident(dep_ident),
                    literal.to_owned(),
                    req.clone(),
                );
            }
        }

        Ok(())
    }
}

/// Rewrite `AssemblyInfo.cs` to include real version numbers.
#[derive(Debug)]
pub struct AssemblyInfoCsRewriter {
    proj_id: ProjectId,
    cs_path: RepoPathBuf,
}

impl AssemblyInfoCsRewriter {
    /// Create a new `AssemblyInfo.cs` rewriter.
    pub fn new(proj_id: ProjectId, cs_path: RepoPathBuf) -> Self {
        AssemblyInfoCsRewriter { proj_id, cs_path }
    }
}

impl Rewriter for AssemblyInfoCsRewriter {
    fn rewrite(&self, app: &AppSession, changes: &mut ChangeList) -> Result<()> {
        let mut did_anything = false;
        let file_path = app.repo.resolve_workdir(&self.cs_path);

        let cur_f = atry!(
            File::open(&file_path);
            ["failed to open file `{}` for reading", file_path.display()]
        );
        let cur_reader = BufReader::new(cur_f);

        let new_af = atomicwrites::AtomicFile::new(
            &file_path,
            atomicwrites::OverwriteBehavior::AllowOverwrite,
        );

        let proj = app.graph().lookup(self.proj_id);

        let r = new_af.write(|new_f| {
            for line in cur_reader.lines() {
                let line = atry!(
                    line;
                    ["error reading data from file `{}`", file_path.display()]
                );

                let line = if line.starts_with("[assembly: AssemblyVersion") || line.starts_with("[assembly: AssemblyFileVersion") {
                    did_anything = true;
                    atry!(
                        crate::pypa::simple_py_parse::replace_text_in_string_literal(&line, &proj.version.to_string());
                        ["couldn't rewrite version-string source line `{}`", line]
                    )
                } else {
                    line
                };

                atry!(
                    writeln!(new_f, "{}", line);
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
                        "rewriter for C# assembly info file `{}` didn't make any modifications",
                        file_path.display()
                    );
                }

                changes.add_path(&self.cs_path);
                Ok(())
            }
        }
    }
}