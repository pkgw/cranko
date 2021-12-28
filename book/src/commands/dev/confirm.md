# `cranko confirm`

Create a new `rc` commit to request the release of one or more projects.

#### Usage

```
cranko confirm [--force]
```

This command gathers release request information prepared from one or more calls
to `cranko stage` and synthesizes it into a new commit on the `rc` branch.
Edited changelog files in the working directory are then reset to match the HEAD
commit.

The `cranko confirm` command analyzes the
[internal interdependencies](../../concepts/internal-dependencies.md) of the
projects within the repository and will refuse to propose a release with
unsatisfied requirements. That is, if a proposed new release of project X would
require a new release of project Y but one is not being requested, the command
will exit with an error.

After the release request is recorded on the `rc` branch, in a typical workflow
the release request would be submitted to the CI/CD system by pushing the branch
to the upstream repository.

#### Example

```shell
$ cranko stage foo_util
foo_util: 4 relevant commit(s) since 1.1.0
$ {edit util/CHANGELOG.md}
$ cranko confirm
info: foo_util: micro bump (expected: 1.1.0 => 1.1.1)
info: staged rc commit to `rc` branch
$ git push origin rc
```
