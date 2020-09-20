[![Build Status](https://dev.azure.com/peter-bulk/Misc/_apis/build/status/pkgw.cranko?branchName=master)](https://dev.azure.com/peter-bulk/Misc/_build/latest?definitionId=2&branchName=master)
[![](https://meritbadge.herokuapp.com/cranko)](https://crates.io/crates/cranko)

# cranko

Cranko is a release automation tool implementing the [just-in-time
versioning][jitv] workflow. It is cross-platform, installable as a
single executable, supports multiple languages and packaging systems, and is
designed from the ground up to work with [monorepos].

[jitv]: https://pkgw.github.io/cranko/book/latest/jit-versioning/
[monorepos]: https://en.wikipedia.org/wiki/Monorepo

To learn more, check out [the book]!

[the book]: https://pkgw.github.io/cranko/book/latest/


## Installation

Cranko is delivered as a single standalone executable for easy installation on
continuous integration systems. On Unix-like systems (including macOS), the
following command will drop an executable named `cranko` in the current
directory:

```shell
curl --proto '=https' --tlsv1.2 -sSf https://pkgw.github.io/cranko/fetch-latest.sh | sh
```

On Windows systems, the following command will do the same in a PowerShell window:

```pwsh
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager::SecurityProtocol -bor 3072
iex ((New-Object System.Net.WebClient).DownloadString('https://pkgw.github.io/cranko/fetch-latest.ps1'))
```

For more details and additional methods, see [the Installation
section][installation] of [the book].

[installation]: https://pkgw.github.io/cranko/book/latest/installation/


## Getting Started

Because Cranko is a workflow tool, to really start using it you’ll need to learn
a bit about how it works and then think about how to integrate it into your
development processes. To learn more, check out the [Getting
Started][getting-started] and [Just-in-Time Versioning][jitv] sections of the
book.

[getting-started]: https://pkgw.github.io/cranko/book/latest/getting-started/


## Development Roadmap

Cranko is still a new project and is lacking many features that would be useful.
Here are some of the things on the radar:

- Built-in support for more languages / frameworks:
  - [x] Rust
  - [ ] NPM
  - [ ] Python
  - [ ] Visual Studio?
- [x] Per-repo config file
  - [x] Identify upstream remote from its URL
  - [ ] Customize project configuration
    - [ ] Custom per-project "rewriter" additions
  - [ ] Defined "unaffiliated" projects
- [x] Revamp error handling infrastructure
- [x] `cranko bootstrap` command to help people onboard
- [ ] Figure out how we're going to make a test suite for this beast

Here are some larger projects that would be cool:

- Split the main implementation into multiple crates
- Pluggable framework for auto-generating release notes (e.g., taking advantage
  of Conventional Commit formats, auto-linking to GitHub pull requests)
- Pluggable framework for knowing when releases should be made and/or
  determining how to bump version numbers (e.g., Conventional Commits plus
  semantic-release type standards)
- Pluggable framework for deciding which commits affect which projects
- Additional templates for release notes, tag names, etc. etc.
- More robust CLI interface for querying the project/release graph so that
  external tools can build on Cranko as a base layer.

## cargo Features

The `cranko` Cargo package provides the following optional [features]:

- `vendored-openssl` — builds the [git2] dependency with its `vendored-openssl`
  feature, which uses a builtin [OpenSSL] library rather than attempting to link
  with the system version. This is useful when cross-compiling because often the
  target environment lacks OpenSSL.

[features]: https://doc.rust-lang.org/cargo/reference/features.html
[git2]: https://crates.io/crates/git2
[OpenSSL]: https://www.openssl.org/


## Contributions

Are welcome! Please open pull requests or issues against the [pkgw/cranko] repository.

[pkgw/cranko]: https://github.com/pkgw/cranko


## Legalities

Cranko copyrights are held by Peter Williams and the Cranko project contributors. Source code is licensed under [the MIT License][mit-license].

[mit-license]: https://opensource.org/licenses/MIT
