# The Cranko Manual

Cranko is a release automation tool implementing the [just-in-time
versioning](jit-versioning/) workflow. It is cross-platform, installable as a
single executable, supports multiple languages and packaging systems, and is
designed from the ground up to work with [monorepos].

[monorepos]: https://en.wikipedia.org/wiki/Monorepo

If you’re just getting started, your first step should probably be to
[install cranko][installation]. Or, check the table of contents to the left if
you’d like to skip directly to a topic of interest.

[installation]: ./installation/index.md


## Contributions are welcome!

This book is a work in progress, and your help is welcomed! The text is written
in [Markdown] (specifically, CommonMark using [pulldown-cmark]) and rendered
into HTML using [mdbook]. The source code lives in the `book/` subdirectory of
[the main Cranko repository]. To make and view changes, all you need to do is
[install mdbook], then run the command:

```sh
$ mdbook serve
```

in the `book/` directory.

[Markdown]: https://commonmark.org/
[pulldown-cmark]: https://crates.io/crates/pulldown-cmark
[mdbook]: https://rust-lang-nursery.github.io/mdBook/
[the main Cranko repository]: https://github.com/pkgw/cranko
[install mdbook]: https://github.com/rust-lang-nursery/mdBook#installation
