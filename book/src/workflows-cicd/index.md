# Cranko CI/CD Workflows

This section focuses on the workflows that should be implemented in your
continuous integration and deployment (CI/CD) system. You can in principle run
those steps outside of the CI/CD context, but the whole point of Cranko is to
automate release processes, so the strong assumption is that these steps will
not be run by humans. In fact, the Cranko commands mentioned in this section
will generally be need to be forced to run *outside* of a CI/CD environment,
which they detect using the [ci_info] Rust crate.

[ci_info]: https://crates.io/crates/ci_info


## Every build

For virtually every build of your repo in your CI/CD infrastructure, the first
thing you should do is [install Cranko](../installation/index.md) (if needed)
and then apply actual version numbers:

```shell
cranko release-workflow apply-versions
```

The Cranko architecture is intended so that your repository should be buildable
without applying versions — because otherwise day-to-day development would be
incredibly tedious — but it is good to apply versions everywhere in CI/CD to
make sure that the relevant plumbing stays in excellent working order.

For pull request builds and merges to the main development branch, you don’t
*need* to do anything more. If you have a continuous deployment scheme that
publishes artifacts with every push to the main branch, you shouldn’t need to
change it. A key thing to keep in mind is that pushes to the main branch, unlike
pushes to `rc`, do not include `cranko confirm` metadata, and so there are no
changelogs and no specific list of projects for which releases are requested.


## `rc` builds

You will need to handle updates to the `rc` branch specially. The initial build
and test process should ideally proceed in exactly the same way as occurs on the
main branch. However, after that process completes, there needs to be a single
decision point that gathers all potential release artifacts and evaluates
whether the build was successful or not. If it failed, there is nothing more to
do. If it was successful, your release deployment automation needs to kick in.

We recommend that this workflow proceed in three stages. First, ensure that all
release artifacts are archived in some fashion. This way, if any later steps
fail, they can be recreated manually.

Next, update the `release` branch, using commands similar to the following:

```shell
$ git add .
$ cranko release-workflow commit
$ git push origin release
```

This “locks in” the release and ensures that any subsequent `rc` submissions do
not try to recreate the releases that your pipeline is about to undertake. The
`commit` command switches the Git repository’s current branch to be `release`,
pointing at the newly created release commit. Commits at the tip of the
`release` branch, like those at the tip of `rc`, contain Cranko metadata. While
`rc` commits contain release *request* metadata, `release` commits contain
metadata about which releases were actually made (and not made).

Finally, perform whichever deployment steps are required: creating GitHub
releases, publishing packages to NPM, updating websites, etc. These operations
do not necessarily need to involve the `cranko` tool at all.

However, when you’re using a monorepo, it is important to keep in mind that each
release involves some unpredictable *subset* of the projects in your repo. The
`cranko` tool can be the source of truth about which projects were just released
and which version numbers they were assigned. Many of the `cranko` commands
beyond the core workflow operations are utilities that leverage Cranko’s
knowledge of the project release graph to ease the implementation of this final
stage of the release process.


## The `release` branch

Your CI/CD system should do *nothing* when the `release` branch is updated. This
branch is only for recording the success of `rc` processing — all of the
interesting stuff should happen there.