# Zenodo Metadata Files

Cranko's [Zenodo integration][zint] involves one or more configuration files,
traditionally named `zenodo.json5`. This page documents the format of these
files.

[zint]: ../integrations/zenodo.md

A project repository may contain zero, one, or many Zenodo metadata files.
Cranko does not care about where they live in the repository tree. So long as
the commands that run in your CI system refer to the right files in the right
places, any filesystem layout is fine.

The Zenodo metadata file is parsed in the [JSON5] format. This is a superset of
[JSON] that is slightly more flexible, especially including support for
comments.

[JSON5]: https://json5.org/
[JSON]: https://en.wikipedia.org/wiki/JSON

The overall structure of the Zenodo metadata file should be as follows:

```
{
  "conceptrecid": $string
  "metadata": $object
}
```

### `conceptrecid`

This field is mandatory. When publishing the first release of a project to
Zenodo, it should contain text of the form `"new-for:$version"`, where
`$version` is the to-be-published version of the project.

After the first release, it should be replaced with the Zenodo “record ID” of
the “concept” item corresponding to the project. This is the serial number
associated with the “Cite all version” item associated with the project. The
[`cranko zenodo preregister`] command will print out this record ID when it runs
for a first release. But don’t worry: it's not hard to figure out this value.

[`cranko zenodo preregister`]: ../commands/cicd/zenodo-preregister.md

The scheme above is intended to make it so that one does not accidentally create
a series of releases that are not properly linked by their concept identifier.
Because the `new-for` mode captures the specific release that it is intended to
be used for, if you forget to update the field, the next release will error out.

### `metadata`

This field is mandatory. It should be filled with Zenodo deposit metadata in
[the JSON format documented by Zenodo][mdformat]. Use whichever fields are
appropriate for your project.

[mdformat]: https://developers.zenodo.org/#deposit-metadata

The following fields will be overwritten by Cranko upon preregistration:

- `title` will be set to `"$projectname $projectversion"`
- `publication_date` will be set to today’s date, as understood by whichever
  computer Cranko is running on
- `version` will be set to `"$projectversion"`

## Preregistration Rewrites

Upon success of the [`cranko zenodo preregister`] command, this file will be
rewritten to include other metadata specific to the deposit being made. These
updates should not be committed to the main branch of your repository, and you
should not depend on any particular keys being available.
