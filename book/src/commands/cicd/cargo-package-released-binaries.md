# `cranko cargo package-released-binaries`

Create archives of the binary files associated with all Rust/[Cargo] projects
that have had new releases.

[Cargo]: https://doc.rust-lang.org/cargo/

#### Usage

```
cranko cargo package-released-binaries
    [--command-name=COMMAND]
    [--reroot=PREFIX]
    --target {TARGET}
    {DEST-DIR} -- [CARGO-ARGS...]
```

This command should be run in CI processing of an update to the `rc` branch,
after the release has been vetted and the release commit has been created. The
current branch should be the `release` branch.

#### Example

```shell
$ cranko cargo package-released-binaries -t $target /tmp/artifacts/ -- build --release

$ cranko cargo package-released-binaries \
  --command-name=cross \
  --reroot=$(pwd) \
  -t $target \
  /tmp/artifacts/ \
  -- build --target=$target --features=vendored-openssl --release
```

For each [Cargo] project known to Cranko that has a new release, this command
creates a `.tar.gz` or Zip archive file of its associated binaries, if they
exist. These archive files are placed in the `{DEST-DIR}` directory
(`/tmp/artifacts`) in the example. These can be publicized as convenient release
artifacts for projects that are delivered as standalone executables.

In order to discover these binaries, Cranko must run `cargo build`, or a similar
command, for each released project. In particular, it must run a Cargo command
that accepts the `--message-format=json` argument and outputs information about
compiler artifacts. Typically, the command of interest would be `cargo build
--release`, in which case the command line to this tool should end with `--
build --release`. However, you might want to include feature flags or other
selectors as appropriate. The `--message-flags=json` argument will be
automatically (and unconditionally) appended.

Unlike [`cranko cargo foreach-released`](./cargo-foreach-released.md), this
command selects projects by passing a `--package=` argument to the subcommand,
rather than changing the starting directory in which it is invoked. This
behavior is needed for the analysis to work when passing through to `cross` (see
below) when there are any Rust packages not rooted at the repository root.

The created archive files will be named according to the format
`{cratename}-{version}-{target}.{format}`. The archive format is `.tar.gz` on
all platforms except Windows, for which it is `.zip`. This format is chosen by
parsing the `-t`/`--target` argument, *not* by examining the host platform
information.

Within the archive files, the executables will be included with no pathing
information. In the typical case that there is a Cargo project named `foo` with
an associated binary also named `foo`, the archive will unpack into a single
file named `foo` or `foo.exe`. If the project contains multiple binaries, the
archive will contain all of them (unless you add a `--bin` option to the Cargo
arguments).

The `--command-name` argument can be used to specify a different command to be
run instead of the default `cargo`. For instance, one might use
`--command-name=cross` for certain operations in a cross-compiled build using
the [rust-embedded/cross] framework.

[rust-embedded/cross]: https://github.com/rust-embedded/cross

The `--reroot` argument can be used to rewrite the paths returned by the build
tool. This extremely specific operation is needed for the [rust-embedded/cross]
framework, which runs inside a Docker container and therefore returns paths that
look like `/target/$arch/debug/...`. The value of this argument is naively
prepended to whatever paths are returned from the tool. In the
[rust-embedded/cross] case, therefore, `--reroot=.` obtains paths that are
meaningful on the build host.
