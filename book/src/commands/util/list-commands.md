# `cranko list-commands`

This command prints out the sub-commands of `cranko` that are available.

#### Usage

```
cranko list-commands
```

#### Example

```
$ cranko list-commands
Currently available "cranko" subcommands:

    confirm
    git-util
    github
    help
    list-commands
    release-workflow
    show
    stage
    status
```

If a command is available in `$PATH` under the name `cranko-extension`, it will
be available as `cranko extension`.
