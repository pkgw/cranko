# `cranko github delete-release`

Delete a [GitHub release][gh-releases] associated with a given tag name.

[gh-releases]: https://docs.github.com/en/github/administering-a-repository/about-releases

#### Usage

```
cranko github delete-release {TAG-NAME}
```

This command deletes the GitHub release associated with `{TAG-NAME}`.

This command is essentially a generic utility that leverages Cranko’s GitHub
integration. It is provided to support use cases that maintain a “continuous
deployment” release on GitHub that is always associated with the latest push to
a branch (such as `master`). In such a use case, on every update to the branch
in question, you’ll want to delete the existing release, then recreate it and
re-populate its artifacts.

Note that this command has no safety checks or “are you sure?” prompts.
