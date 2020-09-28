# `cranko bootstrap`

Bootstrap an existing repository to start using Cranko.

#### Usage

```
cranko bootstrap [--force] [--upstream UPSTREAM-NAME]
```

For detailed usage guidance, see the [Bootstrapping
Workflow](../../workflows-bootstrap/index.md) section.

The `--upstream UPSTREAM-NAME` option specifies the name of the Git remote that
should be considered the canonical “upstream” repository. If unspecified, Cranko
will guess with a preference for the remote named `origin`.

The `--force` option will force the command to proceed even in unexpected
circumstances, such as when the working tree contains modified files.
