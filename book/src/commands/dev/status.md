# `cranko status`

Print out information about unreleased changes in in the HEAD commit of the
repository.

#### Usage

```
cranko status [PROJECT-NAMES]
```

If `{PROJECT-NAMES}` is unspecified, status information is printed about all
projects.

#### Example

```shell
$ cranko status
tcprint: 2 relevant commit(s) since 0.1.1
drorg: 5 relevant commit(s) since 0.3.0
$
```
