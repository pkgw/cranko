# `cranko zenodo publish`

Publish a new [Zenodo deposit](https://help.zenodo.org/), triggering
registration of its DOI.

#### Usage

```
cranko zenodo publish
  [--force] [-f]
  --metadata=JSON5-FILE
```

This command should be run in CI processing of an update to the `rc` branch,
after [`cranko zenodo preregister`] and any invocations of
[`cranko zenodo upload-artifacts`].

[`cranko zenodo preregister`]: ./zenodo-preregister.md
[`cranko zenodo upload-artifacts`]: ./zenodo-upload-artifacts.md

#### Example

```
cranko zenodo publish --metadata=ci/zenodo.json5
```

This will publish the Zenodo deposit whose metadata are tracked in the file
`ci/zenodo.json5`.

#### Remarks

See [the Zenodo integration documentation][zint] for an overview and description
of Cranko's support for Zenodo deposition. See [Zenodo Metadata Files][zconfig]
for a specification of the metadata file used by this command.

[zint]: ../../integrations/zenodo.md
[zconfig]: ../../configuration/zenodo.md

This command requires that the environment variable `ZENODO_TOKEN` has been
set to a Zenodo API token.

This command should only be run during formal releases, and not during pull
requests. Note also that you can choose to *not* run this command in your CI/CD
pipeline, and instead manually publish your Zenodo deposit after review by a
human. That may be tempting, because Zenodo deposits cannot be changed once they
are published. However, our experience is that it is more reliable and more
convenient to fully automate the publication process and fix bugs in that
automation as they arise, rather than including a human in the loop. If releases
and deposits are “cheap”, there’s no problem with superseding them when one
turns out to have a problem.

#### See also

- [Integrations: Zenodo][zint]
- [Configuration: Zenodo Metadata Files][zconfig]
- [`cranko zenodo preregister`]
- [`cranko zenodo upload-artifacts`]
