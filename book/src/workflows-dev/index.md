# Cranko Developer Workflows

This section focuses on the workflows that you might use in the “inner loop” of
your software development process.


## Day-to-day development

If your repository uses Cranko, your standard development practices don’t need
to change. The only thing that’s different is that your version numbers should
all be set to `0.0.0-dev.0` or something similar.

The `cranko status` command will analyze your repository’s commit history since
the last release on the `release` branch. It might tell you:

```shell
$ cranko status
tcprint: 2 relevant commit(s) since 0.1.1
drorg: 5 relevant commit(s) since 0.1.1
$
```

Here, relevance is determined using the prefixing scheme described in the
[Concepts] section. The most release reference point is determined from your
upstream’s release branch (likely `origin/release`), so make sure to `git fetch`
your upstream after a release so that Cranko is comparing to the right thing.

[Concepts]: ../concepts/index.md

If you are working in a monorepo and one project depends on another, you’ll need
to maintain Cranko’s extra versioning metadata. **TODO write me!**


## Requesting releases

When you’re ready to release one or more projects, it’s a two-step process. The
`cranko stage` command will mark projects as release candidates. If run without
arguments, it will use Cranko’s analysis of the repo’s commit history since the
last release to determine which projects need to be staged:

```shell
$ cranko stage
tcprint: 2 relevant commits
drorg: 5 relevant commits
info: 2 of 2 projects staged
$
```

The only actual action taken by this command is to stub each project’s changelog
with a template version bump command and summaries of the commits affecting each
project since the last release. In this example, this looks like:

```shell
$ head tcprint/CHANGELOG.md 
# rc: micro bump

- Add an amazing new feature
- Fix a dastardly bug

# tcprint 0.1.1 (2020-08-27)

```

The placeholder header line `# rc: micro bump` specifies the version bump that
is being requested. At the moment, this just unilaterally defaults to a bump in
the “micro” (AKA “patch”) version number. When the release is finalized, this
placeholder will be replaced with actual release information as seen in the next
stanza.

You can edit the bump type and the actual changelog contents. We view it as
important that the changelog and/or release notes can be reviewed and curated by
a human.

After one or more `stage` operations, you should run `cranko confirm`:

```shell
$ cranko confirm
info: tcprint: micro bump (expected: 0.1.1 => 0.1.2)
info: drorg: micro bump (expected: 0.1.1 => 0.1.2)
info:     internal dep: tcprint >= 0.1.1
info: staged rc commit to `rc` branch
$
```

This will gather up your changelog updates and create a new commit on the `rc`
branch. (Note that these changelog updates do *not* need to be staged into Git
with `git add`.) The changelogs in the working directory will be reset to
whatever HEAD says they should be. The new commit on `rc` bundles up a *release
request*, containing the set of projects intended for release, the way that
their versions should be bumped, and the changelog / release-notes contents.

Your CI/CD system should be set up so that you can trigger release process
simply by running:

```shell
$ git push origin rc
```

You should never need to force-push to this branch. If a release request fails,
you should fix the problem on the main development branch, create a new `rc`
commit, and try again. **TODO** We should add a command to make it easy to
re-use the changelogs from the previous `rc` commit.