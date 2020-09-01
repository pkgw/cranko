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
    visit::EdgeRef,
};
use std::collections::{HashMap, HashSet};

use crate::{
    errors::{Error, Result},
    project::{Project, ProjectBuilder, ProjectId},
    repository::{CommitAvailability, CommitId, ReleaseCommitInfo, RepoHistory, Repository},
};

type OurNodeIndex = NodeIndex<DefaultIx>;

/// A DAG of projects expressing their dependencies.
#[derive(Debug, Default)]
pub struct ProjectGraph {
    /// The projects. Projects are uniquely identified by their index into this
    /// vector.
    projects: Vec<Project>,

    /// NodeIndex values for each project based on its identifier.
    node_ixs: Vec<OurNodeIndex>,

    /// The `petgraph` state expressing the project graph.
    graph: DiGraph<ProjectId, Option<CommitId>>,

    /// Mapping from user-facing project name to project ID. This is calculated
    /// in the complete_loading() method.
    name_to_id: HashMap<String, ProjectId>,
}

impl ProjectGraph {
    /// Get the number of projects in the graph.
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    /// Start the process of adding a new project to the graph.
    pub fn add_project<'a>(&'a mut self) -> ProjectBuilder<'a> {
        if self.name_to_id.len() != 0 {
            panic!("cannot add projects after finalizing initialization");
        }

        ProjectBuilder::new(self)
    }

    // Undocumented helper for ProjectBuilder to finish off its work.
    #[doc(hidden)]
    pub fn finalize_project_addition<F>(&mut self, f: F) -> ProjectId
    where
        F: FnOnce(ProjectId) -> Project,
    {
        let id = self.projects.len();
        self.projects.push(f(id));
        self.node_ixs.push(self.graph.add_node(id));
        id
    }

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
        self.name_to_id.get(name.as_ref()).map(|id| *id)
    }

    /// Add a dependency between two projects in the graph.
    pub fn add_dependency(
        &mut self,
        depender_id: ProjectId,
        dependee_id: ProjectId,
        min_version: Option<CommitId>,
    ) {
        let depender_nix = self.node_ixs[depender_id];
        let dependee_nix = self.node_ixs[dependee_id];
        self.graph.add_edge(dependee_nix, depender_nix, min_version);
    }

    /// Complete construction of the graph.
    ///
    /// In particular, this function calculates unique, user-facing names for
    /// every project in the graph. After this function is called, new projects
    /// may not be added to the graph.
    pub fn complete_loading(&mut self) -> Result<()> {
        // TODO: our algorithm for coming up with unambiguous names is totally
        // ad-hoc and probably crashes in various corner cases. There's probably
        // a much smarter way to approach this.

        let node_ixs = toposort(&self.graph, None).map_err(|cycle| {
            let ident = self.graph[cycle.node_id()];
            Error::Cycle(self.projects[ident].user_facing_name.to_owned())
        })?;

        let name_to_id = &mut self.name_to_id;

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
            fn compute_name(&self, proj: &Project) -> String {
                let mut s = String::new();
                let qnames = proj.qualified_names();
                const SEP: char = ':';

                for i in 0..self.n_narrow {
                    if i != 0 {
                        s.push(SEP);
                    }

                    s.push_str(&qnames[self.n_narrow - 1 - i]);
                }

                s
            }
        }

        let mut states = vec![NamingState::default(); self.projects.len()];
        let mut need_another_pass = true;

        while need_another_pass {
            name_to_id.clear();
            need_another_pass = false;

            for node_ix in &node_ixs {
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
                let qn1 = proj1.qualified_names();
                let qn2 = proj2.qualified_names();
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
                    if n1 > n2 {
                        states[ident1].n_narrow = std::cmp::max(states[ident1].n_narrow, n2 + 1);
                    } else if n2 > n1 {
                        states[ident2].n_narrow = std::cmp::max(states[ident2].n_narrow, n1 + 1);
                    } else {
                        return Err(Error::NamingClash(states[ident1].compute_name(proj1)));
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

        for (name, ident) in name_to_id {
            self.projects[*ident].user_facing_name = name.clone();
        }

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

        for index1 in 1..self.projects.len() {
            let (left, right) = self.projects.split_at_mut(index1);
            let litem = &mut left[index1 - 1];

            for ritem in right {
                litem.repo_paths.make_disjoint(&ritem.repo_paths);
                ritem.repo_paths.make_disjoint(&litem.repo_paths);
            }
        }

        Ok(())
    }

    /// Iterate over all projects in the graph, in no particular order.
    ///
    /// In most cases `toposort()` is preferable, but unlike that function,
    /// this one is infallible.
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
    pub fn toposort_idents(&self) -> Result<impl IntoIterator<Item = ProjectId>> {
        let idents = toposort(&self.graph, None)
            .map_err(|cycle| {
                let ident = self.graph[cycle.node_id()];
                Error::Cycle(self.projects[ident].user_facing_name.to_owned())
            })?
            .iter()
            .map(|ix| self.graph[*ix])
            .collect::<Vec<_>>();
        Ok(idents)
    }

    /// Get an iterator to visit the projects in the graph in topologically
    /// sorted order.
    ///
    /// TODO: this should be superseded by toposort_idents(), it just gets
    /// annoying to hold the ref to the graph.
    ///
    /// That is, if project A in the repository depends on project B, project B
    /// will be visited before project A. This operation is fallible if the
    /// dependency graph contains cycles — i.e., if project B depends on project
    /// A and project A depends on project B. This shouldn't happen but isn't
    /// strictly impossible.
    pub fn toposort(&self) -> Result<GraphIter> {
        let node_idxs = toposort(&self.graph, None).map_err(|cycle| {
            let ident = self.graph[cycle.node_id()];
            Error::Cycle(self.projects[ident].user_facing_name.to_owned())
        })?;

        Ok(GraphIter {
            graph: self,
            node_idxs_iter: node_idxs.into_iter(),
        })
    }

    /// Get an iterator to visit the projects in the graph in topologically
    /// sorted order, mutably.
    ///
    /// See `toposort()` for details. This function is the mutable variant.
    pub fn toposort_mut(&mut self) -> Result<GraphIterMut> {
        let node_idxs = toposort(&self.graph, None).map_err(|cycle| {
            let ident = self.graph[cycle.node_id()];
            Error::Cycle(self.projects[ident].user_facing_name.to_owned())
        })?;

        Ok(GraphIterMut {
            graph: self,
            node_idxs_iter: node_idxs.into_iter(),
        })
    }

    /// Process the query and return a vector of matched project IDs
    pub fn query(&self, query: GraphQueryBuilder) -> Result<Vec<ProjectId>> {
        // Note: while it generally feels "right" to not allow repeated visits
        // to the same project, this is especially important if a query is used
        // to construct a mutable iterator, since it breaks soundness to have
        // such an iterator visit the same project more than once.
        let mut matched_idents = Vec::new();
        let mut seen_ids = HashSet::new();

        // Build up the list of input projids

        let root_idents = if query.no_names() {
            toposort(&self.graph, None)
                .map_err(|cycle| {
                    let ident = self.graph[cycle.node_id()];
                    Error::Cycle(self.projects[ident].user_facing_name.to_owned())
                })?
                .iter()
                .map(|ix| self.graph[*ix])
                .collect::<Vec<_>>()
        } else {
            let mut root_idents = Vec::new();

            for name in query.names {
                if let Some(id) = self.name_to_id.get(&name) {
                    root_idents.push(*id);
                } else {
                    return Err(Error::NoSuchProject(name));
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

    pub fn resolve_direct_dependencies(
        &self,
        repo: &Repository,
        ident: ProjectId,
    ) -> Result<Vec<ResolvedDependency>> {
        let mut deps = Vec::new();

        for edge in self
            .graph
            .edges_directed(self.node_ixs[ident], petgraph::Direction::Incoming)
        {
            let dependee_id = self.graph[edge.source()];
            let dependee_proj = &self.projects[dependee_id];
            let maybe_cid = edge.weight();

            let availability = if let Some(cid) = maybe_cid {
                repo.find_earliest_release_containing(dependee_proj, cid)?
            } else {
                CommitAvailability::NotAvailable
            };

            deps.push(ResolvedDependency {
                ident: dependee_id,
                min_commit: maybe_cid.clone(),
                availability,
            });
        }

        Ok(deps)
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

/// Information about the version requirements of one project's dependency upon
/// another project within the repo. If no version has yet been published
/// satisying the dependency, min_version is None.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDependency {
    pub ident: ProjectId,
    pub min_commit: Option<CommitId>,
    pub availability: CommitAvailability,
}

/// Builder structure for querying projects in the graph.
///
/// The main purpose of this type is to support command-line applications that
/// accept some number of projects as arguments. Depending on the use case, it
/// might be zero or more projects, exactly one project, etc.
#[derive(Debug)]
pub struct GraphQueryBuilder {
    names: Vec<String>,
    release_info: Option<ReleaseCommitInfo>,
    project_type: Option<String>,
}

impl Default for GraphQueryBuilder {
    fn default() -> Self {
        GraphQueryBuilder {
            names: Vec::new(),
            release_info: None,
            project_type: None,
        }
    }
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
        self.names.len() == 0
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
        let mut graph = ProjectGraph::default();
        let mut ids = HashMap::new();

        for (qnames, user_facing) in spec {
            let mut b = graph.add_project();
            b.qnames(*qnames);
            b.version(Version::Semver(semver::Version::new(0, 0, 0)));
            b.prefix(RepoPathBuf::new(b""));
            let projid = b.finish_init();
            ids.insert(projid, user_facing);
        }

        graph.complete_loading()?;

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
