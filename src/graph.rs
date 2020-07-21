// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! The graph of projects within the repository.
//!
//! A Cranko-enabled repository may adopt a “monorepo” model where it contains
//! multiple projects, each with their own independent versioning scheme. The
//! projects will likely all be managed in a single repository because they
//! depend on each other. In the general case, these intra-repository
//! dependencies have the structure of a directed acyclic graph (DAG).

use petgraph::graph::DiGraph;

use crate::project::{Project, ProjectBuilder, ProjectId};

/// A DAG of projects expressing their dependencies.
#[derive(Debug, Default)]
pub struct ProjectGraph {
    /// The projects. Projects are uniquely identified by their index into this
    /// vector.
    projects: Vec<Project>,

    /// The `petgraph` state expressing the project graph.
    graph: DiGraph<ProjectId, ()>,
}

impl ProjectGraph {
    pub fn len(&self) -> usize {
        self.graph.node_count()
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
        id
    }

    /// Get a reference to a project in the graph from its ID.
    pub fn lookup(&self, ident: ProjectId) -> &Project {
        &self.projects[ident]
    }

    /// Get a reference to a project in the graph from its ID.
    pub fn lookup_mut(&mut self, ident: ProjectId) -> &mut Project {
        &mut self.projects[ident]
    }
}
