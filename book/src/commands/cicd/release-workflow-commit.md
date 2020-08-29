# `cranko release-workflow commit`

Commit staged changes to the `release` branch, recording information about new
releases.

#### Usage

```
cranko release-workflow commit [--force]
```

This command should be run in CI processing of an update to the `rc` branch,
after the release has been vetted. The current branch should be the `rc` branch.
This command will switch the current branch to the `release` branch, pointing at
the new release commit.

This command should be run after [`cranko release-workflow
apply-versions`][apply-versions] to create the final `release` commit marking
the successful release of the packages submitted as part of the current `rc`
request. It can be run either before or after the release request is confirmed
to be successful; but if it is run before, care should be taken that the commit
is pushed to the upstream repository *if and only if* the CI tests are
successful.

[apply-versions]: ./release-workflow-apply-versions.md

Unlike [`cranko confirm`](../dev/confirm.md), this command respects the Git
staging workflow, operating like `git commit` itself. Before running this
command, you should first run `git add .` or something similar before it to
stage all changed files. Note that in some workflows, a full build will result
in modifications to files beyond those edited by the [`apply
versions`][apply-versions] command, although ideally this should happen as
minimally as possible. For instance, while Cranko can rewrite a `Cargo.toml`
file for you, it does not attempt to rewrite `Cargo.lock`, which will instead be
updated by the next call to `cargo build` or a similar command. Therefore, you
should make sure that your `git add` command includes both the `Cargo.toml`
*and* the `Cargo.lock` files when staging for the release commit.
