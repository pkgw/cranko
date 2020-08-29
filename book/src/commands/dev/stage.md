# `cranko stage`

Begin the process of preparing one or more projects for release.

#### Usage

```
cranko stage [--force] [PROJECT-NAMES...]
```

If `{PROJECT-NAMES}` is unspecified, all projects that have been affected by any
commits since their last release are staged.

For each project that is staged, its changelog files in the working directory
are rewritten to include template release-request information and a draft set of
release notes based on the Git commits affecting the project since its last
release. The exact format used will depend on the project’s configuration.

You should edit these files as you see fit to prepare the release notes and set
the parameters of the proposed release. The changelog will include previous
entries which can be revised if desired. When the release information is ready,
use `cranko confirm` to prepare a new commit on the `rc` branch for submission
to the CI/CD system.

To “un-stage” a project, just restore its changelog files to their unmodified
state.
