# `cranko show`

The `cranko show` command displays various potentially useful pieces of
information about Cranko, its execution environment, and so on. It provides
several subcommands:

- [`cranko show cranko-concept-doi`](#cranko-show-cranko-concept-doi)
- [`cranko show cranko-version-doi`](#cranko-show-cranko-version-doi)
- [`cranko show if-released`](#cranko-show-if-released)
- [`cranko show tctag`](#cranko-show-tctag)
- [`cranko show toposort`](#cranko-show-toposort)
- [`cranko show version`](#cranko-show-version)


## `cranko show cranko-concept-doi`

This commands prints the [concept DOI](https://help.zenodo.org/) associated with
the Cranko software package.

#### Usage

```
cranko show cranko-concept-doi
```

#### Remarks

The printed [DOI](https://www.doi.org/) is a citeable identifier associated with
Cranko that will never change. Each individual release of Cranko is also
associated with a “version DOI”, which you can use to log the specific version
of Cranko that you used in a particular workflow. Citation metadata link the
different version DOIs through the concept DOI.

You are unlikely to need this command in everyday workflows.


## `cranko show cranko-version-doi`

This commands prints the [DOI](https://www.doi.org/) associated with the currently
running version of Cranko.

#### Usage

```
cranko show cranko-version-doi
```

#### Remarks

Each release of Cranko should have a unique version number as well as a unique
version DOI. While most DOIs resolve to scholarly publications, Cranko version
DOIs “resolve” to a specific release of Cranko, logged with associated metadata
and digital artifacts. If you wish to record the exact version of Cranko that
you used in a workflow in the context of a scholarly citation system, use this
DOI.


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
