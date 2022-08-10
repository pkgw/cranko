# `cranko zenodo upload-artifacts`

Upload files to be associated with an in-progress [Zenodo
deposit](https://help.zenodo.org/).

#### Usage

```
cranko zenodo upload-artifacts
  [--force] [-f]
  --metadata=JSON5-FILE
  FILES[...]
```

This command should be run in CI processing of an update to the `rc` branch,
after [`cranko zenodo preregister`] and before [`cranko zenodo publish`].

[`cranko zenodo preregister`]: ./zenodo-preregister.md
[`cranko zenodo publish`]: ./zenodo-publish.md

#### Example

```
cranko zenodo upload-artifacts --metadata=ci/zenodo.json5 build/mypackage-0.1.0.tar.gz
```

This will upload the file `build/mypackage-0.1.0.tar.gz` and associate it with
the Zenodo deposit whose metadata are tracked in the file `ci/zenodo.json5`.

#### Remarks

See [the Zenodo integration documentation][zint] for an overview and description
of Cranko's support for Zenodo deposition. See [Zenodo Metadata Files][zconfig]
for a specification of the metadata file used by this command.

[zint]: ../../integrations/zenodo.md
[zconfig]: ../../configuration/zenodo.md

This command requires that the environment variable `ZENODO_TOKEN` has been
set to a Zenodo API token.

This command should only be run during formal releases, and not during pull
requests.

#### See also

- [Integrations: Zenodo][zint]
- [Configuration: Zenodo Metadata Files][zconfig]
- [`cranko zenodo preregister`]
- [`cranko zenodo publish`]
