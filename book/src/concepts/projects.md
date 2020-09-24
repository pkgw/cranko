# Projects

A *project* is a thing that is manifested in a series of
[releases](./releases.md), each release assigned a unique
[version](./versions.md). In the Cranko model, each projects’ source materials
are tracked in a repository.

We will sometimes refer to projects as “software,“ but it’s worth emphasizing
that there’s no reason that a project has to consist of computer source code. It
could be a website, a data product, or whatever else. A project might be
associated with some kind of external publishing framework, like an [npm
package] or a [Rust crate], but it doesn’t have to be.

[npm package]: https://docs.npmjs.com/about-packages-and-modules
[Rust crate]: https://doc.rust-lang.org/book/ch07-01-packages-and-crates.html


## Prefixing

Cranko associates each project with a certain prefix inside the repository file
tree. These prefixes can overlap somewhat: for instance, it is very common that
a repository contains a main project at its root, and sub-projects within
subdirectories of that root.

By default, Cranko assumes that files inside of a project’s prefix “belong” to
that project, except when those files “belong” to a project rooted in a more
specific prefix. This mapping is used to assess which commits affect which
projects: if a project is rooted in `crates/log_util`, and a commit alters the
file `crates/log_util/src/color.rs`, that commit is categorized as affecting
that project. A single commit may affect zero, one, or many projects. Cranko
uses this analysis to suggest which projects may be ready for release.
