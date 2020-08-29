# Getting Started

The goal of Cranko is to help you implement a clean, reliable *workflow* for
making releases of the software that you develop. Because Cranko is a workflow
tool, there isn’t one single way to “start using” it — the best way to use it
depends on your *current* release workflow (or lack thereof) and CI
infrastructure.

That being said — once Cranko is integrated into your project, a typical release
process should look like the following. You might periodically run the `cranko
status` command to report on the history of commits since your latest
release(s):

```shell
$ cranko status
cranko: 10 relevant commit(s) since 0.0.3
```

When you're ready to make a release, you’ll run commands like this:

```shell
$ cranko stage
cranko: 12 relevant commits
$ {edit CHANGELOG.md to curate the release notes and set version bump type}
$ cranko confirm
info: cranko: micro bump (expected: 0.0.3 => 0.0.4)
info: staged rc commit to `rc` branch
$ git push origin rc
```

Your Cranko-powered CI pipeline will build the `rc` branch, publish a new
release upon success, and update a special `release` branch. You don't need to
edit any files on your main branch to “resume development”. Instead, if you
resynchronize with the origin you’ll now see:

```shell
$ git fetch origin
[...]
   9fa82ad..8be356d  release      -> origin/release
 * [new tag]         cranko@0.0.4 -> cranko@0.0.4
$ cranko status
cranko: 0 relevant commit(s) since 0.0.4
```

Underpinning Cranko’s operation is the [just-in-time
versioning](../jit-versioning/) workflow. It’s important to understand how it
works to understand how you’ll integrate Cranko into your development and
deployment workflow.
