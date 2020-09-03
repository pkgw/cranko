# rc: micro bump

- Get `cargo package-released-binaries` working with `cross` by adding
  `--command-name` and `--reroot` options. Definitely hacky, but whatcha gonna
  do.
- With the above in hand, start providing sample prebuilt binaries for
  `aarch64-unknown-linux-gnu` and `powerpc64le-unknown-linux-gnu` as a proof of
  concept. These have not actually been tested in practice, though.

# cranko 0.0.17 (2020-09-01)

- cargo: take the "v" out of the binary package name template -- fixes the
  one-liner intallers
- ci: minor tidying

# cranko 0.0.16 (2020-09-01)

- Add `cranko cargo package-released-binaries`
- Refactor CI infrastructure and use the above new command.
- Yet more tuning of the execution-environment code, unbreaking it on pull
  requests and in other contexts.

# cranko 0.0.15 (2020-09-01)

- Oops! Fix the release process when only a subset of packages are being
  released.
- Add a `cargo foreach-released` subcommand
- Tidy up project-graph querying framework and add more features.

# cranko 0.0.14 (2020-08-31)

- Make `--force` actually work for `cranko stage` and `cranko confirm`
- book: a first pass of edits and tidying
- Tidy up some internals that "shouldn't" affect the release pipeline, but I
  want to push a release through to verify that.

# cranko 0.0.13 (2020-08-29)

- Add a bunch of content to the book. (I should start releasing this as project
  separate from the `cranko` crate, but the infrastructure isn’t quite there yet
  …)
- ci: move Windows/GNU to stable instead of nightly

# cranko 0.0.12 (2020-08-28)

- Start working on the book, and wire it up to the GitHub pages site.

# cranko 0.0.10 (2020-08-28)

- Add a draft Windows Powershell install script for CI.

# cranko 0.0.9 (2020-08-27)

- Rename `create-release` to `create-releases`. Still getting into the mindset
  of vectorizing over projects.
- Have `create-releases` be picky about its execution environment. It should
  only be run on a checkout of `release` triggered by an update to `rc`.

# cranko 0.0.8 (2020-08-27)

- Add the "force X.Y.Z" version-bump scheme

# cranko 0.0.7 (2020-08-27)

- Add `cranko git-util reboot-branch`
- Start working on stubbing out a GitHub Pages site to host instant-install
  commands for use in CI pipelines. If I've done this right, this release should
  demonstrate the basic workflow.

# cranko 0.0.6 (2020-08-26)

- Try to get internal dependencies working. The most very basic tests don't
  crash. The code feels quite messy.

# cranko 0.0.5 (2020-08-23)

- Add `github install-credential-helper` and hidden support command `github
  _credential-helper`; this release will help check whether they actually work!

# cranko 0.0.4 (2020-08-23)

- Fix up the analysis of release histories to work properly when projects are not
  all released in lockstep. (I hope. Not yet tested.)
- Fix up checking for repository dirtiness in the stage/confirm workflow: modified
  changelogs should be ignore.
- Lots of general polish to the stage/confirm UX
- Infrastructure: try to simplify/clarify CI pipeline logic with conditionals on
  template parameters

# cranko 0.0.3 (2020-08-22)

- Split `apply` into two separate steps, subcommands of a new `release-workflow`
  command along with `tag`.

# cranko 0.0.2 (2020-08-22)

- Add `github` subcommand with some useful utilities
- Wire those up in the Cranko CI/CD pipeline
- Add logging infrastructure and start making the CLI UI nicer
- Work on adding a few more consistency checks
- Fix changelog updating scheme

# Version 0.0.1 (2020-08-19)

Attempt to publish 0.0.1!
