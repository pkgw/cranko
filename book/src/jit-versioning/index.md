# Just-in-Time Versioning

Cranko implements a release workflow that we call **just-in-time versioning**.
This workflow solves several tricky problems that might bother you about
traditional release processes. On the other hand, they might not! People release
software every day with standard techniques, after all. But if you’ve been
bothered by the lack of rigor in some of your release workflows, just-in-time
versioning might be what you've been looking for.

Just-in-time versioning addresses two particular areas where traditional
practices introduce a bit of sloppiness:

1. In a typical release workflow, you assign a version number to a particular
   commit, publish it to CI, and then deploy it if tests are successful. But
   this is backwards: we shouldn’t bless a commit with a release version until
   *after* it has passed the CI suite.
2. Virtually every software packaging system has some kind of metadata file in
   which you describe your software, including its version number —
   `package.json`, `Cargo.toml`, etc. Because these files must be checked into
   your version control system, you are effectively forced to assign a version
   number to *every* commit, not just the commits that correspond to releases.
   What version number is appropriate for these “in-between” commits?

The discussion below will assume good familiarity with the way that the Git
version control system stores revision history. If you haven’t tried to wrestle
with thinking about your history as a [directed acyclic graph][wp-dag], it might
be helpful to have some references handy.

[wp-dag]: https://en.wikipedia.org/wiki/Directed_acyclic_graph


## The core ideas

Say that you agree that the two points above are indeed problems. How do we
address them?

To address **issue #1**, there’s only one possible course of action: if we want
to make a release, we have to “propose” a commit to the CI system, and only
bless it as a release *after* it passes the test suite.

In a practical workflow we’re probably not going to want to propose every single
commit from the main development branch (which we’ll call `main` here). For our
purposes, it doesn’t particularly matter how commits from `main` are chosen to
be proposed — just that it happens.

Once a commit has been proposed, future proposals should only come from later in
the development history: we don’t want to releases to move backwards. So, the
release proposals are a series of commits … that only moves forward … that’s a
branch! Let’s call it the `rc` branch, for “release candidate”.

Say that we propose releases by pushing to an `rc` branch. Some (hopefully
most!) of those proposals are accepted, and result in releases. How do we
synchronize with the `main` branch and keep everything coherent, especially in
light of **issue #2**?

Just-in-time versioning says: don’t! On the `main` branch, assign
everything a version number of 0.0.0, and never change it. When your CI system
runs on the `rc` branch, before you do anything else, edit your metadata files
to assign the actual version numbers. If the build succeeds, commit those
changes and tag them as your release.

One final elaboration. Because the commits with released version numbers are
never merged back into `main`, they form a series of “stubs” forking off from
the mainline development history. But these releases also form a sequence that,
logically speaking, only moves forward, so it would be nice to preserve them in
some branch-like format as well. In the Git formalism, this is possible if we’re
not afraid to construct our own merge commits. Let’s push each release commit to
a branch called `release`, merging `rc` into `release` but discarding the
`release` file tree in favor of `rc`:

```
  main:     rc:          release:

   M8           /---------R2 (v0.3.0)
   |           /          |
   M7 /------C3           |
   | /       |            |
   M6 /------C2 (failed)  |
   | /       |            |
   M5        |            R1 (v0.2.0)
   |         |           /
   M4 /------C1---------/
   | /       |
   M3        |
   |         |
   M2        |
   |         /
   M1-------/
```

This tactic isn’t strictly necessary for just-in-time versioning concept,
because in principle we can preserve the release commits through Git tags alone.
But it becomes very useful for navigating the release history.


## The workflow in practice

In practice, the just-in-time versioning workflow involves only a handful of
special steps. When a project’s CI/CD pipeline has been set up to support the
workflow, the developer’s workflow for proposing releases is trivial:

1. Choose a commit from `main` and propose it to `rc`.

In the very simplest implementation, this step could as straightforward as
running `git push origin $COMMIT:rc`. For reasons described below, Cranko
implements it with two commands: [`cranko stage`](../commands/dev/stage.md) and
[`cranko confirm`](../commands/dev/confirm.md).

In the CI/CD pipeline, things are hardly more complicated:

1. The first step in any such pipeline is to apply version numbers and create a
   release commit. In Cranko, this is performed with [`cranko release-workflow
   apply-versions`](../commands/cicd/release-workflow-apply-versions.md) and
   [`cranko release-workflow
   commit`](../commands/cicd/release-workflow-commit.md).
2. If the CI passes, the release is “locked in” by pushing to `release`.
   If not, the release commit is discarded.

Cranko provides a lot of other infrastructure to make your life easier, but the
core of the just-in-time versioning workflow is this simple. Importantly: you
don’t need to completely rebuild your development and CI/CD pipelines in order
to adopt Cranko. There are only a small number of new steps, and existing setups
can largely be preserved.


## The monorepo wrinkle

The above discussion is written as if your repository contains one project with
one version number. Cranko was written from the ground up, however, to support
**monorepos** ([monolithic repositories][monorepo]), which we will define as any
repository that (somewhat confusingly) contains *more than one* independently
versioned project. People argue about whether monorepos or, um, single-repos are
better, but, empirically, there are numerous high-profile projects that have
adopted a monorepo model, and once you’ve figured out how to deal with
monorepos, you’ve also solved single-repos.

[monorepo]: https://en.wikipedia.org/wiki/Monorepo

Fortunately, virtually everything described above can be “parallelized” over
multiple projects in a single repository. (Here, a “project” is any item in a
repository that has versioned releases.) Most of the work needed to support
monorepos involves making sure that things like GitHub release entries and tag
names are correctly treated in a per-project fashion, rather than a
per-repository fashion.

In principle, you might be tempted to have one `rc` branch and one `release`
branch for each project in a monorepo. This has an appeal, but it comes with two
problems. First, as the number of projects gets large, so does the number of
branches, which is a bit ugly. Second and more important, separating out
releases by each individual project makes it hard to coordinate releases — and
if multiple projects are being tracked in the same repository it is very likely
*because* releases should be coordinated.

Cranko solves this problem by adding more sophistication to the `rc` and
`release` processing. Pushes to the `rc` branch include metadata that specify a
*set* of projects that are being requested for release. (This is what the
`cranko stage` and `cranko confirm` commands do.) Likewise, updates to `release`
include information about which projects actually were released. It turns out
that pushes to `rc` need to contain metadata anyway, to allow the developer to
specify how the version number(s) should be bumped and release-notes content.

There is one more problem that’s more subtle. If a repo contains multiple
projects, some of them probably depend on one another. If everything on the
`main` branch is versioned at 0.0.0, how do we express the version requirements
of these internal dependencies? We can’t just record those versions in the usual
packaging metadata files, because any tools that need to process these internal
dependencies will reject the version constraints (`foo_cli requires foo_lib >
1.20.0, but found foo_lib = 0.0.0`).

Cranko solves this problem by asking your `main` branch to include a bit of
extra metadata expressing these version requirements as *commit identifiers*
rather than version numbers. The underlying idea is that, because projects are
tracked in the same repository, it should *really* be true that at any given
commit, all of the projects within the repo are mutually compatible. Upon
release time, the required commit identifiers are translated into actual version
number requirements. Part of the stage-and-confirm process implemented by Cranko
ensures that you don’t try to release a new version of a depender project
(`foo_cli` above) that requires an as-yet-unreleased version of its dependee
(`foo_lib`). Cranko even has a special mechanism allowing you to make a single
commit that simulataneouly updates `foo_cli` *and* `foo_lib` *and* expresses
that “`foo_cli` now depends on the version of `foo_lib` from the Git commit that
is being made right now”.
