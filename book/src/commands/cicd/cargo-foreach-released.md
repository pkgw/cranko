# `cranko cargo foreach-released`

Run a Rust [cargo] command for all Rust/Cargo projects that have
had new releases.

[cargo]: https://doc.rust-lang.org/cargo/

#### Usage

```
cranko cargo foreach-released
    [--pause=SECONDS]
    [--command-name=COMMAND]
    [--] [CARGO-ARGS...]
```

This command should be run in CI processing of an update to the `rc` branch,
after the release has been vetted and the release commit has been created. The
current branch should be the `release` branch.

#### Example

```shell
$ cranko cargo foreach-released -- publish --no-verify
```

Note that the name of `cargo` itself should *not* be one of the arguments.
Furthermore, due to the way that Cranko parses its command-line arguments, if
any option flags are to be passed to Cargo, you must precede the whole set of
Cargo options with a double-dash (`--`). The example above would run [`cargo
publish --no-verify`][cargo-publish] for each released package — which is
basically the whole reason that this command exists.

[cargo-publish]: https://doc.rust-lang.org/cargo/commands/cargo-publish.html

Automated publishing requires a Cargo API token. Ideally, such tokens should not
be included in command-line arguments. The [`cargo publish`][cargo-publish]
command can obtain tokens from the `CARGO_REGISTRY_TOKEN` environment variable
(for the [Crates.io] registry) or `CARGO_REGISTRIES_${NAME}_TOKEN` for other
registries. See [the `cargo publish` docs][cargo-publish] for the official
documentation.

[Crates.io]: https://crates.io/

The `--command-name` argument can be used to specify a different command to be
run instead of the default `cargo`. For instance, one might use
`--command-name=cross` for certain operations in a cross-compiled build using
the [rust-embedded/cross] framework.

[rust-embedded/cross]: https://github.com/rust-embedded/cross

The `--pause` argument causes the command to pause for the specified number of
seconds between invocations of `cargo` commands, when more than one command is
to be run. This is aimed at `cargo publish` workflows, where you can encounter
errors if you try to publish several interdependent crates in rapid succession.
The problem appears to be that Crates.io checks the dependency specifications of
crates as they’re published, and if one crate requires a version of another that
was *just* published, the check fails. As of writing we don’t know how much of a
delay is enough to avoid this problem, but the Crates.io index repository is
sometimes updated multiple times in the same minute, so something like thirty
seconds is hopefully sufficient.
