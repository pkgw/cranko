# `cranko npm lerna-workaround`

Rewrite internal version requirements of [npm] projects so that [Lerna] will
understand them.

[npm]: https://npmjs.com/
[Lerna]: https://lerna.js.org/

#### Usage

```
cranko npm lerna-workaround
```

This command will rewrite the `package.json` files of your NPM projects.

#### Example

The [Lerna] tool is somewhat limited in its understanding of [internal
dependencies](../../concepts/internal-dependencies.md) within a repository. If
projects A and B are both at version 0.3,
and project B states a requirement on version 0.3 of project A, Lerna
understands the dependency. However, if project B only requires version 0.2 of
project A, Lerna won't realize that the interdependency is internal. This will
cause its understanding of the project dependency ordering to be incomplete,
potentially leading to build-time errors.

This command can temporarily rewrite your files so that Lerna will correctly
understand the internal dependencies. Once you are done using Lerna, you can use
Git to revert the changes, restoring your packages to be annotated with the
correct dependencies.

A sample CI workflow might look like:

```shell
$ cranko release-workflow apply-versions  # write correct versions
$ git add .
$ cranko release-workflow commit  # save them in a release commit
$ cranko npm lerna-workaround  # write fake dep values to working tree
$ lerna bootstrap  # do Lerna-y stuff
$ lerna run build
    ...
$ lerna run test  # done with Lerna
$ git checkout .  # throw away fake deps
```
