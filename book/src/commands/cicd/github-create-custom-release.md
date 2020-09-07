# `cranko github create-custom-release`

Create a new [GitHub release][gh-releases] with customized metadata. You
probably ought to be using [`cranko github
create-releases`](./github-create-releases.md) instead.

[gh-releases]: https://docs.github.com/en/github/administering-a-repository/about-releases

#### Usage

```
cranko github create-custom-release
  [--draft]
  [--prerelease]
  --name {NAME}
  [--desc {DESC}]
  {TAG-NAME}
```

This command creates a new release on GitHub associated with the tag
`{TAG-NAME}`, which should have already been pushed to the GitHub repository.

You should probably using [`cranko github
create-releases`](./github-create-releases.md) instead of this command. The
`create-releases` command efficiently handles monorepos with multiple packages
that may be released at different times, and it automatically calculates the tag
name, release name, and release description to use for each release. *This*
command should be used only to create GitHub releases that are not associated
with particular projects within the source repository. The motivating use case
is the creation of a special “continuous” GitHub prerelease that is deleted (see
[`cranko github delete-release`](./github-delete-release.md)) and recreated with
each update to a project’s main development branch. Note that this command is
essentially decoupled from Cranko’s project-management infrastructure; all it
does is leverage its GitHub API authentication hooks.

By default, GitHub associates each release with a tarball and zipball of the
repository contents at the time of the release. If you want to associate
additional artifacts, use [`cranko github
upload-artifacts`](./github-upload-artifacts.md) with the `--by-tag` option.

Note that GitHub “draft” releases seem to be treated a bit specially by the API.
If you create a draft release with this command, some other release-related
operations may not work. (If you encounter such a case, please add it to the
documentation here.)
