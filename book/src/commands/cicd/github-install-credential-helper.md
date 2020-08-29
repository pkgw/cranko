# `cranko github install-credential-helper`

Install Cranko as a Git [credential helper][git-credentials] that will return a
[GitHub Personal Access Token (PAT)][gh-pats] stored in the environment variable
`GITHUB_TOKEN`.

[git-credentials]: https://git-scm.com/docs/gitcredentials
[gh-pats]: https://docs.github.com/en/github/authenticating-to-github/creating-a-personal-access-token

#### Usage

```
cranko github install-credential-helper
```

This command modifies the user-global Git configuration file to install Cranko
as a “[credential helper][git-credentials]” program that Git uses to
authenticate with remove servers. This particular credential helper uses the
`GITHUB_TOKEN` environment variable to authenticate.

Nothing about this command is specific to the Cranko infrastructure. It just
comes in handy because Cranko projects need to be able to push to their upstream
repositories from CI/CD, and this is tedious to configure without a helper tool.

Furthermore, the only way in which this command is specific to GitHub is in the
name of the environment variable it references, `GITHUB_TOKEN`.

The installed credential helper is implemented with a hidden sub-command `cranko
github _credential-helper`.
