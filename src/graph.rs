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
    algo::{toposort, Cycle},
    graph::{DefaultIx, DiGraph, NodeIndex},
};

use crate::{
    errors::{Error, Result},
    project::{Project, ProjectBuilder, ProjectId},
};

/// A DAG of projects expressing their dependencies.
#[derive(Debug, Default)]
pub struct ProjectGraph {
    /// The projects. Projects are uniquely identified by their index into this
    /// vector.
    projects: Vec<Project>,

    /// NodeIndex values for each project based on its identifier.
    node_ixs: Vec<NodeIndex<DefaultIx>>,

    /// The `petgraph` state expressing the project graph.
    graph: DiGraph<ProjectId, ()>,
}

impl ProjectGraph {
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    pub fn add_project<'a>(&'a mut self) -> ProjectBuilder<'a> {
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

    /// Get an iterator to visit the projects in the graph in topologically
    /// sorted order.
    ///
    /// That is, if project A in the repository depends on project B, project B
    /// will be visited before project A. This operation is fallible if the
    /// dependency graph contains cycles — i.e., if project B depends on project
    /// A and project A depends on project B. This shouldn't happen but isn't
    /// strictly impossible.
    pub fn toposort<'a>(&'a self) -> Result<GraphTopoSort<'a>> {
        let node_idxs = toposort(&self.graph, None).map_err(|cycle| {
            let ident = self.graph[cycle.node_id()];
            Error::Cycle(self.projects[ident].user_facing_name().to_owned())
        })?;

        Ok(GraphTopoSort {
            graph: self,
            node_idxs_iter: node_idxs.into_iter(),
        })
    }
}

/// An iterator for visiting the projects in the graph in a topologically sorted
/// order.
///
/// That is, if project A in the repository depends on project B, project B will
/// be visited before project A.
pub struct GraphTopoSort<'a> {
    graph: &'a ProjectGraph,
    node_idxs_iter: std::vec::IntoIter<NodeIndex<DefaultIx>>,
}

impl<'a> Iterator for GraphTopoSort<'a> {
    type Item = &'a Project;

    fn next(&mut self) -> Option<&'a Project> {
        let node_ix = self.node_idxs_iter.next()?;
        let ident = self.graph.graph[node_ix];
        Some(self.graph.lookup(ident))
    }
}
