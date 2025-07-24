// Copyright 2021 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Visual Studio C# projects.
//!
//! We currently "manually" update `Properties/AssemblyInfo.cs`.

use anyhow::bail;
use log::{info, warn};
use quick_xml::{events::Event, Reader};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
};

use crate::{
    a_ok_or,
    app::{AppBuilder, AppSession},
    atry,
    config::ProjectConfiguration,
    errors::Result,
    project::{DepRequirement, DependencyTarget, ProjectId},
    repository::{ChangeList, RepoPath, RepoPathBuf, Repository},
    rewriters::Rewriter,
    version::Version,
    write_crlf,
};

/// Framework for auto-loading Visual Studio C# projects from the repository
/// contents.
#[derive(Debug, Default)]
pub struct CsProjLoader {
    dirs_of_interest: HashMap<RepoPathBuf, DirData>,
    vdproj_files: Vec<RepoPathBuf>,
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
            let e = self.dirs_of_interest.entry(dir).or_default();
            e.csproj = Some(repopath.to_owned());
        } else if basename.as_ref() == b"AssemblyInfo.cs" {
            // Hardcode the assumption that we should walk up one directory:
            let (dir, _base) = dirname.pop_sep().split_basename();
            let dir = dir.to_owned();
            let e = self.dirs_of_interest.entry(dir).or_default();
            e.assembly_info = Some(repopath.to_owned());
        } else if basename.ends_with(b".vdproj") {
            self.vdproj_files.push(repopath.to_owned());
        }

        Ok(())
    }

    /// Finalize autoloading any CsProj projects. Consumes this object.
    pub fn finalize(
        mut self,
        app: &mut AppBuilder,
        pconfig: &HashMap<String, ProjectConfiguration>,
    ) -> Result<()> {
        // Scan any vdproj files that might be associated with projects.

        let mut guid_to_vdproj: HashMap<String, Vec<RepoPathBuf>> = HashMap::new();

        for vdproj in self.vdproj_files.drain(..) {
            let p = app.repo.resolve_workdir(&vdproj);
            let f = atry!(
                File::open(&p);
                ["failed to open file `{}`", p.display()]
            );
            let reader = BufReader::new(f);
            let mut guid = None;
            let mut ignore = false;

            for line in reader.lines() {
                let line = atry!(
                    line;
                    ["error reading data from file `{}`", p.display()]
                );

                if line.contains("OutputProjectGuid") {
                    // lines look like: `   "OutputProjectGuid" = "8:{733C84E7-58F2-4E09-AC82-58AFD7E7BDD3}"`
                    // Our GUIDs include the curly braces and are always lowercased since the casing isn't
                    // always consistent in different files.
                    let mut this_guid = extract_braced_text(&line)?.to_owned();
                    this_guid.make_ascii_lowercase();

                    if let Some(prev_guid) = guid.as_ref() {
                        if &this_guid != prev_guid {
                            warn!(
                                "ignoring setup project `{}` that seems to reference multiple key project GUIDs",
                                p.display()
                            );
                            ignore = true;
                        }
                    } else {
                        guid = Some(this_guid);
                    }
                }
            }

            if ignore {
                continue;
            }

            // If no GUID found, silently ignore.
            if let Some(guid) = guid {
                guid_to_vdproj.entry(guid).or_default().push(vdproj);
            }
        }

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
                match xml.read_event_into(&mut buf) {
                    Ok(Event::Start(ref e)) => {
                        state = match e.name().0 {
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
                                t.unescape();
                                ["unable to decode XML text in ProjectGuid of `{}`", p.display()]
                            )
                            .into_owned();
                            g.make_ascii_lowercase();
                            guid = Some(g);
                            state = State::Scanning;
                        }

                        State::NameText => {
                            name = Some(atry!(
                                t.unescape();
                                ["unable to decode XML text in AssemblyName of `{}`", p.display()]
                            ).into_owned());
                            state = State::Scanning;
                        }

                        State::DepGuidText => {
                            let mut g = atry!(
                                t.unescape();
                                ["unable to decode XML text in <Project> of `{}`", p.display()]
                            )
                            .into_owned();
                            g.make_ascii_lowercase();
                            dep_guids.push(g);
                            state = State::Scanning;
                        }

                        State::DepReqText => {
                            let r = atry!(
                                t.unescape();
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

            // Finally we can (try to) register this project.

            let qnames = vec![name.to_owned(), "csproj".to_owned()];

            if let Some(ident) = app.graph.try_add_project(qnames, pconfig) {
                let proj = app.graph.lookup_mut(ident);
                proj.prefix = Some(repodir.to_owned());
                proj.version = Some(version);

                // Auto-register a rewriter to update this package's `AssemblyInfo.cs`.
                let rewrite = AssemblyInfoCsRewriter::new(ident, assembly_info.to_owned());
                proj.rewriters.push(Box::new(rewrite));

                // Any vdproj rewriters?
                if let Some(mut vdprojs) = guid_to_vdproj.remove(&guid) {
                    for vdproj in vdprojs.drain(..) {
                        let rewrite = VdprojRewriter::new(ident, vdproj);
                        proj.rewriters.push(Box::new(rewrite));
                    }
                }

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

/// Rewrite a vdproj (setup installer) to include real version numbers.
#[derive(Debug)]
pub struct VdprojRewriter {
    proj_id: ProjectId,
    vdproj_path: RepoPathBuf,
}

impl VdprojRewriter {
    /// Create a new `AssemblyInfo.cs` rewriter.
    pub fn new(proj_id: ProjectId, vdproj_path: RepoPathBuf) -> Self {
        VdprojRewriter {
            proj_id,
            vdproj_path,
        }
    }
}

impl Rewriter for VdprojRewriter {
    fn rewrite(&self, app: &AppSession, changes: &mut ChangeList) -> Result<()> {
        let mut did_anything = false;
        let file_path = app.repo.resolve_workdir(&self.vdproj_path);

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

        let mut pcode = uuid::Uuid::new_v4().hyphenated().to_string();
        pcode.make_ascii_uppercase();
        let pcode = format!("{{{pcode}}}");
        info!(
            "new Product/PackageCode UUID for `{}`: {}",
            self.vdproj_path.escaped(),
            pcode
        );

        let r = new_af.write(|new_f| {
            let mut seen_product = false;

            for line in cur_reader.lines() {
                let line = atry!(
                    line;
                    ["error reading data from file `{}`", file_path.display()]
                );

                // This is a super janky workaround for the fact that the "Configurations"
                // section of a vdproj can list components with "ProductCode" keys that
                // superficially look like the one that we're trying to replace. In my
                // one sample file, the "Product" section containing the ones that we *do*
                // want to replace comes after "Configurations", and its delimiter is
                // distinctive.
                if line.trim() == "\"Product\"" {
                    seen_product = true;
                }

                let line = if line.contains("\"ProductVersion\" =") {
                    // ProductVersion must have the form `X.Y.Z`; the "revision"
                    // component must be stripped.
                    let prod_vers = proj.version.to_string();
                    let pieces: Vec<_> = prod_vers.split('.').collect();
                    let prod_vers = &pieces[..3].join(".");

                    did_anything = true;
                    atry!(
                        replace_vdproj_text(&line, prod_vers);
                        ["couldn't rewrite version-string source line `{}`", line]
                    )
                } else if seen_product
                    && (line.contains("\"ProductCode\" =") || line.contains("\"PackageCode\" ="))
                {
                    did_anything = true;
                    atry!(
                        replace_vdproj_text(&line, &pcode);
                        ["couldn't rewrite pcode source line `{}`", line]
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
                        "rewriter for vdproj file `{}` didn't make any modifications",
                        file_path.display()
                    );
                }

                changes.add_path(&self.vdproj_path);
                Ok(())
            }
        }
    }
}

pub fn extract_braced_text(line: &str) -> Result<&str> {
    let lc_loc = line.find('{');
    let rc_loc = line.rfind('}');

    match (lc_loc, rc_loc) {
        (Some(lc_idx), Some(rc_idx)) => {
            if lc_idx < rc_idx {
                Ok(&line[lc_idx..=rc_idx])
            } else {
                bail!(
                    "expected braced text in line `{}`, but the braces were confusing",
                    line
                );
            }
        }
        _ => {
            bail!(
                "expected braced text in line `{}`, but didn't find a matched pair",
                line
            );
        }
    }
}

/// This function works on vdproj ProductVersion lines that look like:
///
/// ```
///         "ProductVersion" = "8:6.0.13"
/// ```
///
/// Our goal is to replace the `6.0.13` in this example.
pub fn replace_vdproj_text(line: &str, new_val: &str) -> Result<String> {
    let left_loc = line.rfind(':');
    let right_loc = line.rfind('"');

    let (left_idx, right_idx) = match (left_loc, right_loc) {
        (Some(li), Some(ri)) => (li, ri),
        _ => {
            bail!(
                "expected a vdproj string in line `{}`, but I couldn't understand the syntax",
                line
            );
        }
    };

    let mut replaced = line[..=left_idx].to_owned();
    replaced.push_str(new_val);
    replaced.push_str(&line[right_idx..]);
    Ok(replaced)
}
