// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! State for the Cranko CLI application.

use crate::{
    errors::{Error, Result},
    graph::ProjectGraph,
    project::Project,
    repository::{RepoPath, Repository},
};

/// The main Cranko CLI application state structure.
pub struct AppSession {
    /// The backing repository.
    pub repo: Repository,

    /// The graph of projects contained within the repo.
    graph: ProjectGraph,
}

impl AppSession {
    /// Initialize a new application session.
    ///
    /// Initialization may fail if the environment doesn't associate the process
    /// with a proper Git repository with a work tree.
    pub fn initialize() -> Result<AppSession> {
        let repo = Repository::open_from_env()?;
        let graph = ProjectGraph::default();

        Ok(AppSession { graph, repo })
    }

    /// Get the graph of projects inside this app session, mutably.
    pub fn graph_mut(&mut self) -> &mut ProjectGraph {
        &mut self.graph
    }

    /// Get the graph of projects inside this app session.
    ///
    /// If the graph has not yet been loaded, this triggers processing of the
    /// config file and repository to fill in the graph information, hence the
    /// fallibility.
    pub fn populated_graph(&mut self) -> Result<&ProjectGraph> {
        if self.graph.len() == 0 {
            self.populate_graph()?;
        }

        Ok(&self.graph)
    }

    fn populate_graph(&mut self) -> Result<()> {
        // Start by auto-detecting everything in the repo index.

        let mut cargo = crate::loaders::cargo::CargoLoader::default();

        self.repo.scan_paths(|p| {
            let (dirname, basename) = p.split_basename();
            cargo.process_index_item(dirname, basename);
        })?;

        cargo.finalize(self)?;
        Ok(())
    }
}
