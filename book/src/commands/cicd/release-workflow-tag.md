# `cranko release-workflow tag`

Create Git tags corresponding to the projects that were released in an `rc`
build.

#### Usage

```
cranko release-workflow tag
```

This command should be run in CI processing of an update to the `rc` branch,
after the release has been vetted and the release commit has been created. The
current branch should be the `release` branch.

For every project that was released in this `rc` submission, a new Git version
tag is created according to its tag name format. These tags should then be
pushed to the upstream with `git push --tags`.

#### Example

```shell
$ cranko release-workflow tag
info: created tag cranko@0.0.12 pointing at HEAD (e71c2aa)
```
