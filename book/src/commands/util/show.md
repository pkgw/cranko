# `cranko show`

The `cranko show` command displays various potentially useful pieces of
information about Cranko, its execution environment, and so on. It provides
several subcommands:

- [`cranko show if-released`](#cranko-show-if-released)
- [`cranko show tctag`](#cranko-show-tctag)
- [`cranko show toposort`](#cranko-show-toposort)
- [`cranko show version`](#cranko-show-version)


## `cranko show if-released`

This command prints whether a project was just released. It expects to be run on
a CI system with the `release` branch checked out, after the build has succeeded
and [`cranko release-workflow commit`]()../cicd/release-workflow-commit.md) has
been invoked.

#### Usage

```
cranko show if-released [--exit-code] [--tf] {PROJECT_NAME}
```

Different arguments activate different modes by which the program will indicate
whether the named project was just released.

- `--exit-code`: the program will exit with a success exit code (0 on Unix-like
  systems) if the project *was* released. It will exit with an error exit code
  (1 on Unix-like systems) if the project *was not* released.
- `--tf`: the program will print out the word `true` if the project *was*
  released. It print out the word `false` if the project *was not* released.

At least one such mechanism must be activated.

#### Example

```shell
$ cranko show if-released --tf myproject
false
```

## `cranko show tctag`

This command prints out a `thiscommit:` tag that includes the current date and
some random characters, for easy copy-pasting into Cranko internal-dependency
lines.

#### Usage

```
cranko show tctag
```

#### Example

```shell
$ cranko show tctag
thiscommit:2021-06-03:NmEuWn3
```

## `cranko show toposort`

This command prints out the names of the projects in the repository, one per
line, in topologically-sorted order according to
[internal dependencies](../../concepts/internal-dependencies.md). That is,
the name of a project is only printed after the names of all of its dependencies
in the repo have already been printed. Because dependency cycles are prohibited,
this is always possible. The exact ordering may not be stable, even from one
invocation to the next.

#### Usage

```
cranko show toposort
```

#### Example

```shell
$ cranko show toposort
tectonic_errors
tectonic_status_base
tectonic_io_base
tectonic_engine_xetex
tectonic
```

## `cranko show version`

This command prints out the version assigned to a project.

#### Usage

```
cranko show version {PROJECT_NAME}
```

#### Example

```shell
$ cranko show version foo_lib
0.1.17
```
