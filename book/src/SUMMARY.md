[Overview](index.md)

- [Installation](installation/index.md)
- [Getting Started](getting-started/index.md)
- [Just-in-Time Versioning](jit-versioning/index.md)
- [Workflows]()
  - [Bootstrapping](workflows-bootstrap/index.md)
  - [Everyday Development](workflows-dev/index.md)
  - [CI/CD](workflows-cicd/index.md)
- [Integrations]()
  - [Azure Pipelines](integrations/azure-pipelines.md)
  - [Python](integrations/python.md)
  - [Visual Studio C# Projects](integrations/csproj.md)
  - [Zenodo](integrations/zenodo.md)

# Reference Material

- [Concepts]()
  - [Internal Dependencies](concepts/internal-dependencies.md)
  - [Projects](concepts/projects.md)
  - [Releases](concepts/releases.md)
  - [Versions](concepts/versions.md)
- [Configuration](configuration/index.md)
  - [Zenodo Metadata Files](configuration/zenodo.md)

# CLI Commands

- [Developer Commands]()
  - [cranko bootstrap](commands/dev/bootstrap.md)
  - [cranko confirm](commands/dev/confirm.md)
  - [cranko diff](commands/dev/diff.md)
  - [cranko log](commands/dev/log.md)
  - [cranko stage](commands/dev/stage.md)
  - [cranko status](commands/dev/status.md)
- [CI/CD Commands]()
  - [cranko cargo foreach-released](commands/cicd/cargo-foreach-released.md)
  - [cranko cargo package-released-binaries](commands/cicd/cargo-package-released-binaries.md)
  - [cranko ci-util env-to-file](commands/cicd/ci-util-env-to-file.md)
  - [cranko github create-custom-release](commands/cicd/github-create-custom-release.md)
  - [cranko github create-releases](commands/cicd/github-create-releases.md)
  - [cranko github delete-release](commands/cicd/github-delete-release.md)
  - [cranko github install-credential-helper](commands/cicd/github-install-credential-helper.md)
  - [cranko github upload-artifacts](commands/cicd/github-upload-artifacts.md)
  - [cranko npm foreach-released](commands/cicd/npm-foreach-released.md)
  - [cranko npm install-token](commands/cicd/npm-install-token.md)
  - [cranko npm lerna-workaround](commands/cicd/npm-lerna-workaround.md)
  - [cranko python foreach-released](commands/cicd/python-foreach-released.md)
  - [cranko python install-token](commands/cicd/python-install-token.md)
  - [cranko release-workflow apply-versions](commands/cicd/release-workflow-apply-versions.md)
  - [cranko release-workflow commit](commands/cicd/release-workflow-commit.md)
  - [cranko release-workflow tag](commands/cicd/release-workflow-tag.md)
  - [cranko zenodo preregister](commands/cicd/zenodo-preregister.md)
  - [cranko zenodo publish](commands/cicd/zenodo-publish.md)
  - [cranko zenodo upload-artifacts](commands/cicd/zenodo-upload-artifacts.md)
- [Utility Commands]()
  - [cranko git-util reboot-branch](commands/util/git-util-reboot-branch.md)
  - [cranko help](commands/util/help.md)
  - [cranko list-commands](commands/util/list-commands.md)
  - [cranko show](commands/util/show.md)
