# `cranko github upload-artifacts`

Upload artifact files to be associated with a [GitHub release][gh-releases].

[gh-releases]: https://docs.github.com/en/github/administering-a-repository/about-releases

#### Usage

```
cranko github upload-artifacts
  [--overwrite]
  [--by-tag]
  {PROJECT-NAME} {PATH1 [PATH2...]}
```

This command will upload several local files to GitHub and associate them with
a [GitHub release][gh-releases].

The command operates in two modes. By default, the release that’s modified is
the one associated with the Cranko project `{PROJECT-NAME}`, which is expected
to have been released in the current `rc` run. That release should have been
created by the [`cranko github create-releases`](./github-create-releases.md)
command. In this situation, this command should be run in CI processing of an
update to the `rc` branch, after the release has been vetted and the release
commit has been created. The current branch should be the `release` branch.

Alternatively, if the `--by-tag` option is given, the `{PROJECT-NAME}` argument
is treated as a Git tag name that will be looked up directly on GitHub. This
mode is useful if you are trying to upload artifacts associated with a release
created with [`cranko github
create-custom-release`](./github-create-custom-release.md). In this case, the
notion of the “current release” is not necessary, so Cranko’s checks for the
state of the environment are not invoked.

This command assumes that a [GitHub Personal Access Token (PAT)](gh-pats) is
available in an environment variable named `GITHUB_TOKEN`.

[gh-pats]: https://docs.github.com/en/github/authenticating-to-github/creating-a-personal-access-token

Because it does not make sense for this command to parallelize over released
projects, it has relatively few tie-ins with the Cranko infrastructure. The key
touch-point is how, in the default mode, this command uses the Cranko release
information and project name to know which Git tag the artifact files should be
associated with.

#### Example

```shell
# `rc` branch; we know that project foo_data has been released
$ cranko github create-releases foo_data
$ cranko github upload-artifacts foo_data compiled_v1.dat compiled_v2.data
```
