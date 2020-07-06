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

use crate::project::{Project, ProjectId};

/// A DAG of projects expressing their dependencies.
#[derive(Debug, Default)]
pub struct ProjectGraph {
    /// The `petgraph` state expressing the project graph.
    graph: DiGraph<ProjectId, ()>,
}

impl ProjectGraph {
    pub fn len(&self) -> usize {
        self.graph.node_count()
    }

    pub fn add_project(&mut self, proj: &Project) {
        self.graph.add_node(proj.ident());
    }
}
