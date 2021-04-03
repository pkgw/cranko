# `cranko log`

Print repository history for a project since its last release.

#### Usage

```
cranko log [--stat] [PROJECT-NAME]
```

You can leave `[PROJECT-NAME]` unspecified if there's only one project in the
repo.

The `--stat` argument, if specified, is forwarded to `git show`.

#### Example

```shell
$ cranko log
commit d262b397eae451e23c68438fb3ddde6fc64dc65a (HEAD)
Author: Peter Williams <peter@newton.cx>
Date:   Sat Apr 3 10:47:18 2021 -0400

...
```

#### Remarks

This command is helpful to get an overview of the changes that might potentially
be [staged](./stage.md) for a release. It generates a list of relevant commits
and then farms out the display work to the `git show` subcommand.
