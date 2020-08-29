# `cranko github upload-artifacts`

Upload artifact files to be associated with a [GitHub release][gh-releases].

[gh-releases]: https://docs.github.com/en/github/administering-a-repository/about-releases

#### Usage

```
cranko github upload-artifacts [--overwrite] {PROJECT-NAME} {PATH1 [PATH2...]}
```

This command should be run in CI processing of an update to the `rc` branch,
after the release has been vetted and the release commit has been created. The
current branch should be the `release` branch.

This command will upload several local files to GitHub and associate them with
the [GitHub release][gh-releases] associated with a project that has been
released in the current `rc` run. That release should have been created by the
[cranko github create-releases](./github-create-releases.md) command.

This command assumes that a [GitHub Personal Access Token (PAT)](gh-pats) is
available in an environment variable named `GITHUB_TOKEN`.

[gh-pats]: https://docs.github.com/en/github/authenticating-to-github/creating-a-personal-access-token

Because it does not make sense for this command to parallelize over released
projects, it has relatively few tie-ins with the Cranko infrastructure. The key
touch-point is how this command uses the Cranko release information and project
name to know which Git tag the artifact files should be associated with.

#### Example

```shell
# `rc` branch; we know that project foo_data has been released
$ cranko github create-releases foo_data
$ cranko github upload-artifacts foo_data compiled_v1.dat compiled_v2.data
```
