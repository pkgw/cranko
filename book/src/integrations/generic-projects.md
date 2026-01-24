# Integrations: Generic Projects

Cranko supports “generic” projects that are not associated with any
particular external tool.

This capability is useful if you want to apply versioning and release automation
to an asset that doesn’t use versioning-aware developer tools, such as a
document or simple static website.


## Autodetection

Cranko identifies generic projects by looking for directories containing a file
named `CrankoProject.toml`.


## Project Metadata

The format of the `CrankoProject.toml` file is as per this example:

```toml
name = "project-name"
# after versions are applied, a `version = "1.2.3"` entry will be added.

[[rewrite]]
version_placeholder = "<dev>"
files = ["templates/footer.html"]
```

The `name` field specifies the project name as Cranko will report it.

There is no version field, because Cranko manages it. The project’s version will
follow [semver] semantics.

[semver]: https://semver.org/

Each `rewrite` block specifies a set of files that are rewritten using a
particular set of replacement methods. You can specify multiple `rewrite` blocks
if different sets of files need to use different rewriting rules.

Currently, only one style of rewrite block is supported. The block must have a
field `version_placeholder` specifying some fixed placeholder text. For every
file in the repository named in the `files` list (relative to the
`CrankoProject.toml` file defining it), that placeholder will be replaced with
the Cranko-managed project version. In the example above, a line looking like:

```
This is myproject version <dev>.
```

will become something like

```
This is myproject version 1.0.0.
```


## Internal Dependencies

[“Internal” dependencies](../concepts/internal-dependencies.md) are not
currently supported for generic projects.