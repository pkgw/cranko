# The Cranko Bootstrapping Workflow

Cranko provides a special [`cranko bootstrap`](../commands/dev/bootstrap.md)
command to help you start using Cranko with a preexisting repository.


## Invocation

Ideally, to bootstrap a repository to use Cranko all you need to do is enter its
working tree and run:

```shell
$ cranko bootstrap
```

Go ahead and try it! It will try to print out detailed information about what
it’s doing. Since you must run the program in a Git repository working tree, if
it does anything that you don’t like you can always reset your working tree to
throw away the tool’s changes.

Hopefully the tool won’t crash, but these are early days and everyone’s repo is
unique. If you have problem not addressed in the text below, [file an
issue][issue].

[issue]: https://github.com/pkgw/cranko/issues/new


## Guessing the upstream

Cranko needs to know the identity of your upstream repository, which is defined
as the one that will perform automated release processing upon updates to its
`rc`-like branch. The bootstrapper will begin by attempting to guess the
identity of upstream by looking for a Git remote named `origin`, or choosing the
only remote if there is only one. If this guessing process fails, use the
`--upstream` option to specify the name of the upstream explicitly.

The bootstrapper will save the URL of the upstream remote into [the main Cranko
configuration file](../configuration/index.md) `.config/cranko/config.toml`. You
may want to add additional likely upstream URLs to this configuration file
(e.g., both HTTPS and SSH GitHub remote URLs). Cranko identifies the upstream
from its URL, not its Git remote name, since Git remote names can vary
arbitrarily from one checkout to the next.


## Autodetecting projects

The bootstrapper will search for recognized projects in the repo and print out a
summary of what it finds.

**NOTE:** *The goal is for Cranko to recognize all sorts of projects, but
currently it knows a modest group of them: Rust/Cargo, NPM, and Python. If
you’re giving Cranko a first try this is the limitation that is most likely to
be a dealbreaker. Please [file an
issue](https://github.com/pkgw/cranko/issues/new) reporting your needs so we
know what to prioritize.*

**ALSO:** *There is a further goal that one day you’ll be able to manually
configure projects that aren’t discovered in the autodetection phase, but that
functionality is also not yet implemented.*


## Resetting versions

As per the [just-in-time versioning][jitv] workflow, on the main development
branch of your repository, the version numbers of all projects should be set to
some default “development” value (e.g. `0.0.0-dev.0`) that is never planned to
change. Cranko will rewrite all of the metadata files that it recognizes to
perform this zeroing.

[jitv]: ../jit-versioning/index.md

But you’re presumably not going to want to *actually* reset the versioning of
all your projects. The current version numbers will be preserved in a “bootstrap”
configuration file (`.config/cranko/bootstrap.toml`) that Cranko will use as a
basis for assigning new version numbers.


## Transforming internal dependencies

If your repository contains more than one project, some of those projects
probably depend on each other. With zeroed-out version numbers, it is not
generally possible to express the version constraints of those internal
dependencies in existing metadata formats. For instance, before bootstrapping,
you might have had a package `foo_cli` that depends on `foo_lib >= 1.3`: it
works if linked against `foo_lib` version 1.3.0, but not if linked against
`foo_lib` version 1.2.17. That didn’t stop being true just because the version
numbers in on your main development branch got zeroed out!

The boostrapping process transfers your preexisting internal dependency version
requirements into extra Cranko metadata fields so that they will be correctly
reproduced in new releases. Once you start making releases that depend on newer
versions of your projects, it is recommended that you transition these “manually”
coded version requirements to Cranko-native ones based in Git commit identifiers
(as motivated in the [just-in-time versioning][jitv] section).


# Next steps

Once the bootstrapper has run, you should review the changes it has made, see if
they make sense, and try building the code in your repo. You may need to modify
your build scripts depending on what expectations they have about the version
numbers assigned in your main development branch.

After you are happy with Cranko’s changes, commit them, making sure to add the
new files in `.config/cranko/`.

The next step is to modify your CI/CD system to start using the `cranko
release-workflow` commands to start implementing the [just-in-time
versioning][jitv] model — see the [CI/CD Workflows][ci-cd-wf] section for
documentation on what to do. This phase generally takes some trial-and-error,
but in most cases you should only need to insert a few extra commands into your
CI/CD scripts at first. Generally, it is easiest to start by updating the
processes that run on updates to the main development branch (e.g. `master`) and
on pull requests. If you do this work on a branch other than your main
development branch, make sure that your Cranko-ified CI/CD scripts will run on
updates to that branch.

[ci-cd-wf]: ../workflows-cicd/index.md

As you work on the CI/CD configuration for main development work, you probably
won’t actually need to use any of the Cranko commands described in the [Everyday
Development][dev-wf] section. But once your basic processing is working, you
should start using those commands to simulate releases and work on setting up
the CI/CD workflows that run on updates to the new `rc` branch that you will be
creating — these are the workflows that will actually run the automated release
machinery if/when your builds succeed. If you haven’t been using release
automation before, it can take some patience to set everything up properly. But,
we hope that you still soon start feeling the warm fuzzies that arise when these
usually annoying tasks start Just Working!

[dev-wf]: ../workflows-def/index.md
