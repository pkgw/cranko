// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! The graph of projects within the repository.
//!
//! A Cranko-enabled repository may adopt a “monorepo” model where it contains
//! multiple projects, each with their own independent versioning scheme. The
//! projects will likely all be managed in a single repository because they
//! depend on each other. In the general case, these intra-repository
//! dependencies have the structure of a directed acyclic graph (DAG).

use petgraph::{
    algo::toposort,
    graph::{DefaultIx, DiGraph, NodeIndex},
};
use std::collections::{HashMap, HashSet};
use thiserror::Error as ThisError;

use crate::{
    a_ok_or, atry,
    config::ProjectConfiguration,
    errors::Result,
    project::{
        DepRequirement, Dependency, DependencyBuilder, DependencyTarget, Project, ProjectBuilder,
        ProjectId,
    },
    repository::{ReleaseCommitInfo, RepoHistory, Repository},
};

type OurNodeIndex = NodeIndex<DefaultIx>;

/// A DAG of projects expressing their dependencies.
#[derive(Debug, Default)]
pub struct ProjectGraph {
    /// The projects. Projects are uniquely identified by their index into this
    /// vector.
    projects: Vec<Project>,

    /// The `petgraph` state expressing the project graph.
    graph: DiGraph<ProjectId, ()>,

    /// Mapping from user-facing project name to project ID. This is calculated
    /// in the complete_loading() method.
    name_to_id: HashMap<String, ProjectId>,

    /// Project IDs in a topologically sorted order.
    toposorted_ids: Vec<ProjectId>,
}

/// An error returned when an input has requested a project with a certain name,
/// and it just doesn't exist.
#[derive(Debug, ThisError)]
#[error("no such project with the name `{0}`")]
pub struct NoSuchProjectError(pub String);

impl ProjectGraph {
    /// Get a reference to a project in the graph from its ID.
    pub fn lookup(&self, ident: ProjectId) -> &Project {
        &self.projects[ident]
    }

    /// Get a mutable reference to a project in the graph from its ID.
    pub fn lookup_mut(&mut self, ident: ProjectId) -> &mut Project {
        &mut self.projects[ident]
    }

    /// Get a project ID from its user-facing name.
    ///
    /// None indicates that the name is not found.
    pub fn lookup_ident<S: AsRef<str>>(&self, name: S) -> Option<ProjectId> {
        self.name_to_id.get(name.as_ref()).copied()
    }

    /// Iterate over all projects in the graph, in no particular order.
    ///
    /// In many cases [[`Self::toposorted`]] may be preferable.
    pub fn projects(&self) -> GraphIter {
        GraphIter {
            graph: self,
            node_idxs_iter: self
                .graph
                .node_indices()
                .collect::<Vec<OurNodeIndex>>()
                .into_iter(),
        }
    }

    /// Get an iterator to visit the project identifiers in the graph in
    /// topologically sorted order.
    ///
    /// That is, if project A in the repository depends on project B, project B
    /// will be visited before project A. This operation is fallible if the
    /// dependency graph contains cycles — i.e., if project B depends on project
    /// A and project A depends on project B. This shouldn't happen but isn't
    /// strictly impossible.
    pub fn toposorted(&self) -> TopoSortIdentIter {
        TopoSortIdentIter {
            graph: self,
            index: 0,
        }
    }

    /// Get an iterator to visit the projects in the graph in topologically
    /// sorted order, mutably.
    ///
    /// See `toposort()` for details. This function is the mutable variant.
    pub fn toposorted_mut(&mut self) -> TopoSortIterMut {
        TopoSortIterMut {
            graph: self,
            index: 0,
        }
    }

    /// Process the query and return a vector of matched project IDs.
    ///
    /// If one of the specified project names does not correspond to a project,
    /// the returned error will be downcastable to a NoSuchProjectError.
    pub fn query(&self, query: GraphQueryBuilder) -> Result<Vec<ProjectId>> {
        // Note: while it generally feels "right" to not allow repeated visits
        // to the same project, this is especially important if a query is used
        // to construct a mutable iterator, since it breaks soundness to have
        // such an iterator visit the same project more than once.
        let mut matched_idents = Vec::new();
        let mut seen_ids = HashSet::new();

        // Build up the list of input projids

        let root_idents = if query.no_names() {
            self.toposorted_ids.clone()
        } else {
            let mut root_idents = Vec::new();

            for name in query.names {
                if let Some(id) = self.name_to_id.get(&name) {
                    root_idents.push(*id);
                } else {
                    return Err(NoSuchProjectError(name).into());
                }
            }

            root_idents
        };

        // Apply filters and deduplicate if needed

        for id in root_idents {
            let proj = &self.projects[id];

            // only_new_releases() filter
            if let Some(ref rel_info) = query.release_info {
                if rel_info.lookup_if_released(proj).is_none() {
                    continue;
                }
            }

            // only_project_type() filter
            if let Some(ref ptype) = query.project_type {
                let qnames = proj.qualified_names();
                let n = qnames.len();

                if n < 2 {
                    continue;
                }

                if &qnames[n - 1] != ptype {
                    continue;
                }
            }

            // not rejected -- keep this one
            if seen_ids.insert(id) {
                matched_idents.push(id);
            }
        }

        Ok(matched_idents)
    }

    pub fn analyze_histories(&self, repo: &Repository) -> Result<RepoHistories> {
        Ok(RepoHistories {
            histories: repo.analyze_histories(&self.projects[..])?,
        })
    }
}

/// This type is how we "launder" the knowledge that the vector that
/// comes out of repo.analyze_histories can be mapped into ProjectId values.
#[derive(Clone, Debug)]
pub struct RepoHistories {
    histories: Vec<RepoHistory>,
}

impl RepoHistories {
    /// Given a project ID, look up its history
    pub fn lookup(&self, projid: ProjectId) -> &RepoHistory {
        &self.histories[projid]
    }
}

/// Builder structure for querying projects in the graph.
///
/// The main purpose of this type is to support command-line applications that
/// accept some number of projects as arguments. Depending on the use case, it
/// might be zero or more projects, exactly one project, etc.
#[derive(Debug, Default)]
pub struct GraphQueryBuilder {
    names: Vec<String>,
    release_info: Option<ReleaseCommitInfo>,
    project_type: Option<String>,
}

impl GraphQueryBuilder {
    /// Specify particular project names as part of the query.
    ///
    /// Depending on the nature of the query, a zero-sized list may be OK here.
    pub fn names<T: std::fmt::Display>(&mut self, names: impl IntoIterator<Item = T>) -> &mut Self {
        self.names = names.into_iter().map(|s| s.to_string()).collect();
        self
    }

    /// Specify that only projects released in the associated info should be
    /// matched.
    pub fn only_new_releases(&mut self, rel_info: ReleaseCommitInfo) -> &mut Self {
        self.release_info = Some(rel_info);
        self
    }

    /// Specify that only projects with the associated type should be matched.
    pub fn only_project_type<T: std::fmt::Display>(&mut self, ptype: T) -> &mut Self {
        self.project_type = Some(ptype.to_string());
        self
    }

    /// Return true if no input names were specified.
    pub fn no_names(&self) -> bool {
        self.names.is_empty()
    }
}

/// A builder for the project graph upon app startup.
///
/// We do not impl Default even though we could, because the only way to
/// create one of these should be via the AppBuilder.
#[derive(Debug)]
pub struct ProjectGraphBuilder {
    /// The projects. Projects are uniquely identified by their index into this
    /// vector.
    projects: Vec<ProjectBuilder>,

    /// NodeIndex values for each project based on its identifier.
    node_ixs: Vec<OurNodeIndex>,

    /// The `petgraph` state expressing the project graph.
    graph: DiGraph<ProjectId, ()>,
}

/// An error returned when the internal project graph has a dependency cycle.
/// The inner value is the user-facing name of a project involved in the cycle.
#[derive(Debug, ThisError)]
#[error("detected an internal dependency cycle associated with project {0}")]
pub struct DependencyCycleError(pub String);

/// An error returned when it is impossible to come up with distinct names for
/// two projects. This "should never happen", but ... The inner value is the
/// clashing name.
#[derive(Debug, ThisError)]
#[error("multiple projects with same name `{0}`")]
pub struct NamingClashError(pub String);

impl ProjectGraphBuilder {
    pub(crate) fn new() -> ProjectGraphBuilder {
        ProjectGraphBuilder {
            projects: Vec::new(),
            node_ixs: Vec::new(),
            graph: DiGraph::default(),
        }
    }

    /// Request to register a new project with the graph.
    ///
    /// The request may be denied if the user has specified that
    /// the project should be ignored.
    pub fn try_add_project(
        &mut self,
        qnames: Vec<String>,
        pconfig: &HashMap<String, ProjectConfiguration>,
    ) -> Option<ProjectId> {
        // Not the most elegant ... I can't get join() to work here due to the
        // rev(), though.

        let mut full_name = String::new();

        for term in qnames.iter().rev() {
            if !full_name.is_empty() {
                full_name.push(':')
            }

            full_name.push_str(term);
        }

        let ignore = pconfig
            .get(&full_name)
            .map(|c| c.ignore)
            .unwrap_or_default();
        if ignore {
            return None;
        }

        let mut pbuilder = ProjectBuilder::new();
        pbuilder.qnames = qnames;

        let id = self.projects.len();
        self.projects.push(pbuilder);
        self.node_ixs.push(self.graph.add_node(id));
        Some(id)
    }

    /// Get a mutable reference to a project buider from its ID.
    pub fn lookup_mut(&mut self, ident: ProjectId) -> &mut ProjectBuilder {
        &mut self.projects[ident]
    }

    /// Add a dependency between two projects in the graph.
    pub fn add_dependency(
        &mut self,
        depender_id: ProjectId,
        dependee_target: DependencyTarget,
        literal: String,
        req: DepRequirement,
    ) {
        self.projects[depender_id]
            .internal_deps
            .push(DependencyBuilder {
                target: dependee_target,
                literal,
                cranko_requirement: req,
                resolved_version: None,
            });
    }

    /// Complete construction of the graph.
    ///
    /// In particular, this function calculates unique, user-facing names for
    /// every project in the graph. After this function is called, new projects
    /// may not be added to the graph.
    ///
    /// If the internal project graph turns out to have a dependecy cycle, an
    /// error downcastable to DependencyCycleError.
    pub fn complete_loading(mut self) -> Result<ProjectGraph> {
        // The first order of business is to determine every project's
        // user-facing name. TODO: our algorithm for this is totally ad-hoc and
        // probably crashes in various corner cases. There's probably a much
        // smarter way to approach this.

        let mut name_to_id = HashMap::new();

        // Each project has a vector of "qualified names" [n1, n2, ..., nN] that
        // should be unique. Here n1 is the "narrowest" name and probably
        // corresponds to what the user naively thinks of as the project names.
        // Farther-out names help us disambiguate, e.g. in a monorepo containing
        // a Python project and an NPM project with the same name. Our
        // disambiguation simply strings together n_narrow items from the narrow
        // end of the list. If qnames is [foo, bar, bax, quux] and n_narrow is
        // 2, the rendered name is "bar:foo".
        #[derive(Copy, Clone, Debug, Eq, PartialEq)]
        struct NamingState {
            pub n_narrow: usize,
        }

        impl Default for NamingState {
            fn default() -> Self {
                NamingState { n_narrow: 1 }
            }
        }

        impl NamingState {
            fn compute_name(&self, proj: &ProjectBuilder) -> String {
                let mut s = String::new();
                const SEP: char = ':';

                for i in 0..self.n_narrow {
                    if i != 0 {
                        s.push(SEP);
                    }

                    s.push_str(&proj.qnames[self.n_narrow - 1 - i]);
                }

                s
            }
        }

        let mut states = vec![NamingState::default(); self.projects.len()];
        let mut need_another_pass = true;

        while need_another_pass {
            name_to_id.clear();
            need_another_pass = false;

            for node_ix in &self.node_ixs {
                use std::collections::hash_map::Entry;
                let ident1 = self.graph[*node_ix];
                let proj1 = &self.projects[ident1];
                let candidate_name = states[ident1].compute_name(proj1);

                let ident2: ProjectId = match name_to_id.entry(candidate_name) {
                    Entry::Vacant(o) => {
                        // Great. No conflict.
                        o.insert(ident1);
                        continue;
                    }

                    Entry::Occupied(o) => o.remove(),
                };

                // If we're still here, we have a name conflict that needs
                // solving. We've removed the conflicting project from the map.
                //
                // We'd like to disambiguate both of the conflicting entries
                // equally. I.e., if the qnames are [pywwt, npm] and [pywwt,
                // python] we want to end up with "python:pywwt" and
                // "npm:pywwt", not "python:pywwt" and "pywwt".

                let proj2 = &self.projects[ident2];
                let qn1 = &proj1.qnames;
                let qn2 = &proj2.qnames;
                let n1 = qn1.len();
                let n2 = qn2.len();
                let mut success = false;

                for i in 0..std::cmp::min(n1, n2) {
                    if qn1[i] != qn2[i] {
                        success = true;
                        states[ident1].n_narrow = std::cmp::max(states[ident1].n_narrow, i + 1);
                        states[ident2].n_narrow = std::cmp::max(states[ident2].n_narrow, i + 1);
                        break;
                    }
                }

                if !success {
                    use std::cmp::Ordering;

                    match n1.cmp(&n2) {
                        Ordering::Greater => {
                            states[ident1].n_narrow =
                                std::cmp::max(states[ident1].n_narrow, n2 + 1);
                        }
                        Ordering::Less => {
                            states[ident2].n_narrow =
                                std::cmp::max(states[ident2].n_narrow, n1 + 1);
                        }
                        Ordering::Equal => {
                            return Err(NamingClashError(states[ident1].compute_name(proj1)).into());
                        }
                    }
                }

                if name_to_id
                    .insert(states[ident1].compute_name(proj1), ident1)
                    .is_some()
                {
                    need_another_pass = true; // this name clashes too!
                }

                if name_to_id
                    .insert(states[ident2].compute_name(proj2), ident2)
                    .is_some()
                {
                    need_another_pass = true; // this name clashes too!
                }
            }
        }

        // Now that we've figured out names, convert the ProjectBuilders into
        // projects. resolving internal dependencies and filling out the graph.
        //

        let mut projects = Vec::with_capacity(self.projects.len());

        for (ident, mut proj_builder) in self.projects.drain(..).enumerate() {
            // TODO: more lame linear indexing.
            let mut name = None;

            for (i_name, i_ident) in &name_to_id {
                if *i_ident == ident {
                    name = Some(i_name.clone());
                    break;
                }
            }

            let name = name.unwrap();
            let mut internal_deps = Vec::with_capacity(proj_builder.internal_deps.len());
            let depender_nix = self.node_ixs[ident];

            for dep in proj_builder.internal_deps.drain(..) {
                let dep_ident = match dep.target {
                    DependencyTarget::Ident(id) => id,
                    DependencyTarget::Text(ref dep_name) => *a_ok_or!(
                        name_to_id.get(dep_name);
                        ["project `{}` states a dependency on an unrecognized project name: `{}`",
                         name, dep_name]
                    ),
                };

                internal_deps.push(Dependency {
                    ident: dep_ident,
                    literal: dep.literal,
                    cranko_requirement: dep.cranko_requirement,
                    resolved_version: dep.resolved_version,
                });

                let dependee_nix = self.node_ixs[dep_ident];
                self.graph.add_edge(dependee_nix, depender_nix, ());
            }

            let proj = proj_builder.finalize(ident, name, internal_deps)?;
            projects.push(proj);
        }

        // Now that we've done that and compiled all of the interdependencies,
        // we can verify that the graph has no cycles. We compute the
        // topological sorting once and just reuse it later.

        let sorted_nixs = atry!(
            toposort(&self.graph, None).map_err(|cycle| {
                let ident = self.graph[cycle.node_id()];
                DependencyCycleError(projects[ident].user_facing_name.to_owned())
            });
            ["the project graph contains a dependency cycle"]
        );

        let toposorted_ids = sorted_nixs
            .iter()
            .map(|node_ix| self.graph[*node_ix])
            .collect();

        // Another bit of housekeeping: by default we set things up so that
        // project's path matchers are partially disjoint. In particular, if
        // there is a project rooted in prefix "a/" and a project rooted in
        // prefix "a/b/", we make it so that paths in "a/b/" are not flagged as
        // belonging to the project in "a/".
        //
        // The algorithm here (and in make_disjoint()) is not efficient, but it
        // shouldn't matter unless you have an unrealistically large number of
        // projects. We have to use split_at_mut() to get simultaneous
        // mutability of two pieces of the vec.

        for index1 in 1..projects.len() {
            let (left, right) = projects.split_at_mut(index1);
            let litem = &mut left[index1 - 1];

            for ritem in right {
                litem.repo_paths.make_disjoint(&ritem.repo_paths);
                ritem.repo_paths.make_disjoint(&litem.repo_paths);
            }
        }

        // All done

        Ok(ProjectGraph {
            projects,
            name_to_id,
            graph: self.graph,
            toposorted_ids,
        })
    }
}

/// An iterator for visiting the graph's pre-toposorted list of idents.
///
/// This type only exists to provide the convenience of an iterator over
/// this toposorted list that (a) doesn't clone the whole vec, by holding
/// a ref to the graph, but (b) yields ProjectIds, not &ProjectIds.
pub struct TopoSortIdentIter<'a> {
    graph: &'a ProjectGraph,
    index: usize,
}

impl<'a> Iterator for TopoSortIdentIter<'a> {
    type Item = ProjectId;

    fn next(&mut self) -> Option<ProjectId> {
        if self.index < self.graph.toposorted_ids.len() {
            let rv = self.graph.toposorted_ids[self.index];
            self.index += 1;
            Some(rv)
        } else {
            None
        }
    }
}

/// An iterator for visiting the toposorted projects in the graph, mutably.
pub struct TopoSortIterMut<'a> {
    graph: &'a mut ProjectGraph,
    index: usize,
}

impl<'a> Iterator for TopoSortIterMut<'a> {
    type Item = &'a mut Project;

    fn next(&mut self) -> Option<&'a mut Project> {
        // Here we have a classic case where a naive implemention runs afoul of
        // the borrow checker. It thinks that our return value can only have a
        // lifetime as long as the lifetime of the `&mut self` reference, which
        // is shorter than 'a. However, if all of the indexes generated by our
        // iter are unique -- and they are -- we can safely "upgrade" the
        // returned lifetime since it won't allow multiple aliasing to the same
        // project over the course of the iteration. The unsafe bit that allows
        // this. Cf:
        // https://users.rust-lang.org/t/help-with-iterators-yielding-mutable-references/24892
        if self.index < self.graph.toposorted_ids.len() {
            let ident = self.graph.toposorted_ids[self.index];
            self.index += 1;
            Some(unsafe { &mut *(self.graph.lookup_mut(ident) as *mut _) })
        } else {
            None
        }
    }
}

/// An iterator for visiting the projects in the graph.
pub struct GraphIter<'a> {
    graph: &'a ProjectGraph,
    node_idxs_iter: std::vec::IntoIter<OurNodeIndex>,
}

impl<'a> Iterator for GraphIter<'a> {
    type Item = &'a Project;

    fn next(&mut self) -> Option<&'a Project> {
        let node_ix = self.node_idxs_iter.next()?;
        let ident = self.graph.graph[node_ix];
        Some(self.graph.lookup(ident))
    }
}

/// An iterator for visiting the projects in the graph, mutably.
pub struct GraphIterMut<'a> {
    graph: &'a mut ProjectGraph,
    node_idxs_iter: std::vec::IntoIter<OurNodeIndex>,
}

impl<'a> Iterator for GraphIterMut<'a> {
    type Item = &'a mut Project;

    fn next(&mut self) -> Option<&'a mut Project> {
        let node_ix = self.node_idxs_iter.next()?;
        let ident = self.graph.graph[node_ix];

        // Here we have a classic case where a naive implemention runs afoul of
        // the borrow checker. It thinks that our return value can only have a
        // lifetime as long as the lifetime of the `&mut self` reference, which
        // is shorter than 'a. However, if all of the indexes generated by
        // node_idx_iter are unique -- and they are -- we can safely "upgrade"
        // the returned lifetime since it won't allow multiple aliasing to the
        // same project over the course of the iteration. The unsafe bit that
        // allows this. Cf:
        // https://users.rust-lang.org/t/help-with-iterators-yielding-mutable-references/24892
        Some(unsafe { &mut *(self.graph.lookup_mut(ident) as *mut _) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{repository::RepoPathBuf, version::Version};

    fn do_name_assignment_test(spec: &[(&[&str], &str)]) -> Result<()> {
        let mut graph = ProjectGraphBuilder::new();
        let mut ids = HashMap::new();
        let empty_config = HashMap::new();

        for (qnames, user_facing) in spec {
            let qnames = qnames.iter().map(|s| (*s).to_owned()).collect();
            let projid = graph.try_add_project(qnames, &empty_config).unwrap();
            let b = graph.lookup_mut(projid);
            b.version = Some(Version::Semver(semver::Version::new(0, 0, 0)));
            b.prefix = Some(RepoPathBuf::new(b""));
            ids.insert(projid, user_facing);
        }

        let graph = graph.complete_loading()?;

        for (projid, user_facing) in ids {
            assert_eq!(graph.lookup(projid).user_facing_name, *user_facing);
        }

        Ok(())
    }

    #[test]
    fn name_assignment_1() {
        do_name_assignment_test(&[(&["A", "B"], "A")]).unwrap();
    }

    #[test]
    fn name_assignment_2() {
        do_name_assignment_test(&[(&["A", "B"], "B:A"), (&["A", "C"], "C:A")]).unwrap();
    }

    #[test]
    fn name_assignment_3() {
        do_name_assignment_test(&[
            (&["A", "B"], "B:A"),
            (&["A", "C"], "C:A"),
            (&["D", "B"], "D"),
            (&["E"], "E"),
        ])
        .unwrap();
    }

    #[test]
    fn name_assignment_4() {
        do_name_assignment_test(&[(&["A", "A"], "A:A"), (&["A"], "A")]).unwrap();
    }

    #[test]
    fn name_assignment_5() {
        do_name_assignment_test(&[
            (&["A"], "A"),
            (&["A", "B"], "B:A"),
            (&["A", "B", "C"], "C:B:A"),
            (&["A", "B", "C", "D"], "D:C:B:A"),
        ])
        .unwrap();
    }
}
