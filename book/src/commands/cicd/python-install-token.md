# `cranko python install-token`

Install a [PyPI authentication token][pypi-token] into the per-user `.pypirc`
configuration file to enable the publishing of Python packages to [PyPI].

[pypi-token]: https://pypi.org/help/#apitoken
[PyPI]: https://pypi.org/

#### Usage

```
cranko python install-token [--repository=REPO]
```

This command appends the user-global python configuration file `.pypirc` to
include an authentication token from the environment variable `PYPI_TOKEN`. The
default `REPO` is `pypi`.

Nothing about this command is specific to the Cranko infrastructure. It just
comes in handy because publishing to PyPI is a common release automation task,
and there aren’t many good ways to get a credential like `$PYPI_TOKEN` from the
environment into a file without exposing it on the command line of a program.

For maximum security, the `.pypirc` file should be destroyed with a tool like
`shred` after it is no longer needed.
