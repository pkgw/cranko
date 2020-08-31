# `cranko release-workflow apply-versions`

Edit the files in the working tree to apply the version numbers requested in the
current `rc` release request.

#### Usage

```
cranko release-workflow apply-versions [--force]
```

This command should be run as early as possible in all forms of your CI/CD
pipeline. It will rewrite your project metadata files (`package.json`,
`Cargo.toml`, etc.) to apply new version numbers as needed. On pushes to the
`rc` branch, if the CI test suite passes, a final release commit should be
created with [`cranko release-workflow commit`](./release-workflow-commit.md)
and then pushed to the upstream `release` branch to “lock in” the requested
releases.

For each project, new versions are computed by applying a “bump specification” to
the version logged in the metadata of the most recent commit on the `release`
branch. If that branch does not exist, and for newly-created projects, the
reference version defaults to `0.0.0` or its equivalent. For pushes to the `rc`
branch, projects whose releases have been requested have bumps applied based on
the metadata of the `rc` release request. In other cases — such as PRs or pushes
to the main development branch — all project versions are bumped using the
default ”development mode” scheme, which usually applies a datecode or some
other kind of informal identifier. Artifacts built in this mode should not be
released openly.
