# `cranko github create-releases`

Create new [GitHub releases][gh-releases] associated with all projects that have
had releases.

[gh-releases]: https://docs.github.com/en/github/administering-a-repository/about-releases

#### Usage

```
cranko github create-releases [PROJECT-NAMES...]
```

This command should be run in CI processing of an update to the `rc` branch,
after the release has been vetted and the release commit has been created. The
current branch should be the `release` branch.

If `{PROJECT-NAMES}` is unspecified, creates releases for all projects that were
released in this run. Otherwise, creates releases only for the name projects,
*if* they have been released in this run. If an unreleased project is named, a
warning is issued and the project is ignored.

The GitHub releases are identified by the project name and have their
description populated with the project release notes. By default, GitHub
associates each release with a tarball and zipball of the repository contents at
the time of the release. If you want to associate additional artifacts, use
[cranko github upload-artifacts](./github-upload-artifacts.md).
