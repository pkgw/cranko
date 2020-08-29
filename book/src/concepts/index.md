# Cranko Concepts

Let’s go over a few concepts and terms central to Cranko’s operation.


## Assumptions

The fundamental assumption of Cranko is that we are seeking to achieve total
automation of the software release process. It is important to point out that
Cranko does not seek to automate the *decision to release*: it is the authors’
opinion that it is important for this decision to be in human hands. (Although
if you want to automate that decision too, we can’t and won’t stop you.) But
once that decision has been made, as much of the process involved should proceed
mechanically. We believe that the [just-in-time versioning][jitv] workflow
provides an extremely sound basis on which to make this happen.

[jitv]: ../jit-versioning/index.md

Cranko assumes that your source materials are stored in a Git repository. Cranko
could probably in principle be ported to other systems, but there are no plans
to do so. As described in the [just-in-time versioning][jitv] section, Cranko’s
underlying operation model does assume that the repository history is easily
expressible as a [DAG], that branches and merges are cheap, and so on.

[DAG]: https://en.wikipedia.org/wiki/Directed_acyclic_graph

It is worth emphasizing that Cranko tries *not* assume that your software is
implemented in a particular language or runs on a particular architecture. While
different packaging systems need explicit support within Cranko, the fundamental
model is intended to be extremely general.


## Projects

Cranko operates on Git repositories. Each repository is understood to contain
one or more *projects*, where a project is basically any derivative product of
the repository to which you might want to assign a version number. We will
sometimes refer to projects as “software,“ but it’s worth emphasizing that
there’s no reason that a project has to consist of computer source code. It
could be a website, a data product, or whatever else. A project might be
associated with some kind of external publishing framework, like an [npm
package] or a [Rust crate], but it doesn’t have to be.

[npm package]: https://docs.npmjs.com/about-packages-and-modules
[Rust crate]: https://doc.rust-lang.org/book/ch07-01-packages-and-crates.html

Many repositories contain just one project. There are plenty of repositories
that contain multiple projects, which will — somewhat awkwardly from a
linguistic standpoint — be our definition of a [monorepo] (monolithic
repository). It’s important to keep the monorepo case in mind because it can be
tempting to talk about “the version of the repository” or “making a release of
the repository,” and these are not well-defined concepts when you have multiple
projects in a single repository. (Well, you could have a rule that all projects
in the same repo must have the same version, like [lerna]’s “non-independent”
mode, but this is an extremely restrictive and problematic policy to adopt.)
Cranko is designed for the monorepo case, since if you support that correctly
you, of course, will support the non-monorepo (single-repo?) case too.

[monorepo]: https://en.wikipedia.org/wiki/Monorepo
[lerna]: https://lerna.js.org/


## Releases

Cranko’s idea of a *release* closely tracks the one implied by the [semantic
versioning specification][semver]. Each project in a repo is sent out into the
world in a time-series of releases. Each release is associated with a version, a
Git commit, and some set of *artifacts*, which are almost always “files” in the
standard computing sense. All of these should be immutable and, to some extent,
irrevocable: once you’ve made a release, it’s never coming back. This sounds
dangerous, but a big point of release automation is to make the release process
so easy that if you mess something up, it’s easy to supersede it with a fixed
version.

[semver]: https://semver.org/

Cranko’s model takes pains to avoid strong assumptions about what version
“numbers” look like — they don't even need to be numbers — or how they change
over time. Well-specified versioning syntaxes like [semver] are supported, but
the goal is to make it possible to use domain-specific syntaxes as well.

Because repositories can contain multiple projects, an individual commit in a
repository’s history might be associated with zero, one, or *many* project
releases. This model requires a certain amount of trust: if I release project X
in commit Y, I’m implictly asserting that all projects not-X do *not* need to be
released at the same time. There is no way for a computer to know that this is
actually true. (The same kind of trust is required by Git’s merge algorithm,
which assumes that if two different commits do not alter the same part of the
same files, that they do not conflict with one another. This assumption is a
good heuristic, but not infallible.) In Cranko’s case, the only way to avoid
placing this trust in the user would be to demand that the release of *any*
project requires the release of *all* projects, which is takes cautiousness to
the level of absurdity.


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