# `cranko npm install-token`

Install an [NPM authentication token][npm-token] into the per-user `.npmrc`
configuration file to enable the publishing of NPM packages.

[npm-token]: https://docs.npmjs.com/about-authentication-tokens

#### Usage

```
cranko npm install-token [--registry=REGISTRY]
```

This command appends the user-global NPM configuration file `.npmrc` to include
an authentication token from the environment variable `NPM_TOKEN`. The default
`REGISTRY` is `//registry.npmjs.org/`.

Nothing about this command is specific to the Cranko infrastructure. It just
comes in handy because publishing to NPM is a common release automation task,
and there aren’t many good ways to get a credential like `$NPM_TOKEN` from the
environment into a file without exposing it on the command line of a program.

For maximum security, the `.npmrc` file should be destroyed with a tool like
`shred` after it is no longer needed.
