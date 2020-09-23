# `cranko npm foreach-released`

Run a command for all [npm] projects that have had new releases.

[npm]: https://npmjs.com/

#### Usage

```
cranko npm foreach-released [--] [COMMAND...]
```

This command should be run in CI processing of an update to the `rc` branch.

#### Example

```shell
$ cranko npm foreach-released -- npm publish
```

This would run [`npm publish`][npm-publish] for each released package — which is
basically the whole reason that this command exists. The command is run “for”
each package in the sense that the initial directory of each executed command is
the directory containing the package’s `package.json` file.

[npm-publish]: https://docs.npmjs.com/cli/publish

Automated publishing requires an NPM registry authentication token. Such a token
can be securely installed into the per-user `.npmrc` configuration file with
[`cranko npm install-token`](./npm-install-token.md).
