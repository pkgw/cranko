# Introduction

This book describes the Cranko release automation tool, which implements the
[just-in-time versioning](../jit-versioning/) workflow. It is cross-platform,
installable as a single executable, supports multiple languages and packaging
systems, and is designed from the ground up to work with monorepos. It requires
that your projects are tracked in Git repositories and *strongly* benefits from
tight integration with a continuous integration and deployment (CI/CD) system.


## Getting Started

The goal of Cranko is to help you implement a clean, reliable *workflow* for
making releases of the software that you develop. Because Cranko is a workflow
tool, there isn’t one single way to “start using” it — the best way to use it
depends on your *current* release workflow (or lack thereof) and CI
infrastructure.

That being said — once Cranko is integrated into your project, a typical release
process should look like the following. You might periodically run the `cranko
status` command to report on the history of commits since your latest
release(s):

```
$ cranko status
cranko: 10 relevant commit(s) since 0.0.3
```

When you're ready to make a release, you’ll run commands like this:

```
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

```
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


## Installation

Cranko is installable as a single binary executable installable in several ways:

- On a Unix-like operating system (Linux or macOS), the following command will
  place the latest release of the `cranko` executable into the current directory:
  ```
  curl --proto '=https' --tlsv1.2 -sSf https://pkgw.github.io/cranko/fetch-latest.sh | sh
  ```
  If your CI/CD environment doesn't make Cranko available in a more standardized
  way, this is the recommended installation command.
- On Windows, the following PowerShell commands will do the same:
  ```
  [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager::SecurityProtocol -bor 3072
  iex ((New-Object System.Net.WebClient).DownloadString('https://pkgw.github.io/cranko/fetch-latest.ps1'))
  ```
- You can manually download precompiled binaries from the Cranko [GitHub release
  archive][github-releases].
- If you have a [Rust] toolchain installed, you can compile and install your own
  version with `cargo install cranko`.

[github-releases]: https://github.com/pkgw/cranko/releases/latest
[Rust]: https://www.rust-lang.org/tools/install

Note that, to fully implement the just-in-time versioning workflow, the `cranko`
command will need to be available both on your development machine and on the
machines implementing your CI/CD pipeline.