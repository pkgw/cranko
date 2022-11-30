# `cranko npm install-token`

Install an [NPM authentication token][npm-token] into the per-user `.npmrc`
or `.yarnrc.yml` configuration file to enable the publishing of NPM packages.

[npm-token]: https://docs.npmjs.com/about-authentication-tokens

#### Usage

```
cranko npm install-token [--yarn] [--registry=REGISTRY]
```

This command appends a user-global configuration file to include an
authentication token from the environment variable `NPM_TOKEN`.

By default, the configuration is targeted at the `npm` command: the `.npmrc`
file is edited, and the default `REGISTRY` is `//registry.npmjs.org/`.

If the `--yarn` option is specified, the `.yarnrn.yml` file is instead edited,
and the default `REGISTRY` is `https://registry.yarnpkg.com/`. Note that in this
mode the name of the input environment variable is still `NPM_TOKEN`. The same
token will work with Yarn, but needs to be placed in this different file in
order for the `yarn npm publish` command to work.

Nothing about this command is specific to the Cranko infrastructure. It just
comes in handy because publishing to NPM is a common release automation task,
and there aren’t many good ways to get a credential like `$NPM_TOKEN` from the
environment into a file without exposing it on the command line of a program.

For maximum security, the `.npmrc` or `.yarnrc.yml` file should be destroyed
with a tool like `shred` after it is no longer needed.
