# Internal Dependencies

An *internal dependency* is a dependency between two [projects](./projects.md)
stored in the same repository. Internal dependencies are therefore closely
associated with [monorepos] in Cranko's terminology. You can have a monorepo
that doesn't have any internal dependencies, but usually the point of a monorepo
setup is to manage a group of interdependent projects.

[monorepos]: https://en.wikipedia.org/wiki/Monorepo

As outlined in the introduction to [Just-in-Time Versioning][jitv-int-deps],
internal dependencies (perhaps counterintuitively) take some extra effort to
manage. This situation stems from two assumptions in the JIT model:

[jitv-int-deps]: ../jit-versioning/index.md#the-monorepo-wrinkle

- Any intra-project dependency (internal or external) needs to be associated
  with a *version requirement* specifying the range of versions of the
  "dependee" package that the "depending" package is compatible with. In simple
  cases this might be expressible as "anything newer than 1.0", but version
  requirements can potentially be complex ("anything in the 1.x or 2.x series,
  but not 3.x, and not 1.10").
- In the JIT versioning model, specific version numbers shouldn't be stored in
  the main development branch of your repository.

It's important to note that dependency version requirements can't be determined
automatically. Say that I have a monorepo containing two projects,
`awesome_database` and `awesome_webserver`. It's reasonable to assume that at
any given commit, the two are compatible, but is the development version of
`awesome_webserver` compatible with version 1.9 of `awesome_database`? Is it
compatible with version 1.1? You could imagine some level of automated API
analysis to test source-level compatibility, but it's always possible that the
semantics of an API can change in a way that maintains source compatibility but
breaks actual usage. Ultimately the *only* sound approach is for a human to make
this determination.

Getting back to Cranko's challenge: at some point I'm going to want to make a
new release of `awesome_webserver` with metadata saying that it requires
`awesome_database >= 2.0`. How can I tell Cranko what version requirement to
insert into the `awesome_webserver` project files when the main development
branch can't "know" what the most recent version of `awesome_database` is?

## Commit-Based Internal Dependency Version Requirements

Cranko solves this problem by requiring that you specify internal dependency
version requirements as *Git commits*, not version numbers. For each internal
dependency from a "depending" project X on a "dependee" project Y, you must
specify a Git commit such that X is compatible with releases of Y whose sources
contain that commit in their histories.

What does Cranko do with this information? When making a release of project X,
Cranko has sufficient information to determine the *oldest* version of project Y
containing that commit. It will rewrite project X's public metadata to encode
that version requirement.

It can happen that no such release exists — perhaps project X requires a new
feature that was just added to Y, and no release of Y has yet been made. Cranko
will detect this situation and, correctly, refuse to let you make a release of
project X. However, you can release X and Y *simultaneously* (in one `rc` push),
and Cranko will detect this and generate correct metadata.

The commit-based model implies a restriction that version requirements for
internal dependencies must have the simplest form: X is compatible with any
version of Y newer than Z, for some Z determined at release time. This is not
expected to be restrictive in practice because Cranko assumes that at any given
commit in a monorepo, all projects are compatible as expressed in the source
tree.

## Expressing Internal Dependency Commit Requirements

Project meta-files don't have native support for commit-based version
requirements because it would be inappropriate to include information specific
to a project's revision system in such files. Therefore, Cranko always has to
define some kind of custom way for you to capture this metadata, with the
specific mechanism depending on the project type. For instance:

- In Rust, you add `[package.metadata.internal_dep_versions]` fields in Cargo.toml
- In Python, you annotate version requirement lines in your `setup.py` file or
  equivalent

The documentation for each language integration should specify the approach and
specific syntax you should use.

## Development with Internal Dependency Requirements

The [`cranko bootstrap`][bs] command will endeavor to update your project files
to include your pre-existing internal version requirements using a special
"manual" mode. This is required for version requirements that reach into a
project's pre-Cranko history.

[bs]: ../workflows-bootstrap/index.md#transforming-internal-dependencies

Once the internal requirements are set up, you should *ideally* update commit
requirements as APIs are added or broken. For instance, say that project
`awesome_database` adds a new API in commit A, and project `awesome_webserver`
starts using it sometime later in commit B. Commit B *should* update the
metadata to indicate that `awesome_webserver` now requires a version of
`awesome_database` based on commit A, or later.

If you don't remember to update the metadata immediately, that's OK. So long as
the metadata for `awesome_webserver` are updated sometime before its next
release, the released files will contain the right information.
