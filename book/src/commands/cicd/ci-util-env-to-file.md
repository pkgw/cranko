# `cranko ci-util env-to-file`

Write the contents of an environment variable to a file, securely.

#### Usage

```
cranko ci-util env-to-file
  [--decode=[text,base64]]
  {VAR-NAME} {FILE-PATH}
```

This command examines the value of an environment variable `{VAR-NAME}` and
writes it to a file on disk at `{FILE-PATH}`. Many CI systems expose credentials
and other secret values as environment variables, and sometimes one needs to get
these values into a file on disk for use by an external program. This tool
provides a relatively secure mechanism for doing so, because it avoids inserting
the variable’s value into the command-line arguments of an external program,
which is generally unavoidable when trying to accomplish this effect within a
shell script.

#### Example

```shell
$ cranko ci-util env-to-file --decode=base64 SECRET_KEY_BASE64 secret.key
```

Note that the variable name is written undecorated, without a leading `$` or
wrapping `%%`. This is vital! Otherwise your shell will expand the value of the
variable before running the command, which will not only cause it to fail, but
will defeat the whole goal of the command, which is to avoid revealing the
variable’s value on the terminal.

The `--decode` option specifies how the value of the variable should be decoded
before writing to disk. In the default, `text`, the variable’s value is treated
as Unicode text, in whatever standard is most appropriate for the operating
system, and written to the file in UTF-8 encoding. If the mode is `base64`, the
variable’s value is taken to be base64-encoded text, and the decoded binary data
are written out.

The file on disk is created in “exclusive” mode, such that the tool will exit
with an error if the file already exists. On Unix systems, it is created such
that only the owning user has any access permissions (mode 0o600).

Files created with this tool should be scrubbed off of the filesystem after they
are no longer needed with an approprite utility such as `shred`.