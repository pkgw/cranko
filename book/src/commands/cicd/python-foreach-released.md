# `cranko python foreach-released`

Run a command for all [PyPA] projects that have had new releases.

[PyPA]: https://www.pypa.io/

#### Usage

```
cranko python foreach-released [--] [COMMAND...]
```

This command should be run in CI processing of an update to the `rc` branch.

#### Example

```shell
$ cranko python foreach-released -- touch upload-me.txt
```

This would run the command `touch upload-me.txt` for each released Python
package. The command is run “for” each package in the sense that the initial
directory of each executed command is the directory containing the package’s
project meta-files.

Note that this command is not so useful because the recommended PyPA publishing
command, [`twine upload`], needs to be passed the name of the distribution
file(s) to upload, and this Cranko command currently doesn’t give you a
convenient way to interpolate those names. This feature isn’t fully baked
because we’re unaware of any examples of single repositories containing multiple
Python projects, so “vectorization” over all Python releases isn’t needed. For
now, check whether your Python project was released using [`cranko show
if-released`], and run its publishing commands manually.

[`twine upload`]: https://twine.readthedocs.io/en/latest/#twine-upload
[`cranko show if-released`]: ../util/show.md#cranko-show-if-released