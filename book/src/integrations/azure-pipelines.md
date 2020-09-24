# Integrations: Azure Pipelines

The [Azure Pipelines][ap-home] CI/CD service is a great match for Cranko because
its ability to divide builds into [stages][ap-stages] that exchange
[artifacts][ap-artifacts] works very nicely with Cranko’s model for CI/CD
processing. This section will go over ways that you can use Cranko in the Azure
Pipelines framework.

[ap-home]: https://azure.microsoft.com/en-us/services/devops/pipelines/
[ap-stages]: https://docs.microsoft.com/en-us/azure/devops/pipelines/process/stages?view=azure-devops
[ap-artifacts]: https://docs.microsoft.com/en-us/azure/devops/pipelines/artifacts/artifacts-overview?view=azure-devops


## Examples

Here are some projects that use Cranko in Azure Pipelines:

- [Cranko itself](https://github.com/pkgw/cranko/tree/master/ci)
- [pkgw/elfx86exts](https://github.com/pkgw/elfx86exts/tree/master/ci), a simple
  single-crate project
- [tectonic-typesetting/tectonic](https://github.com/tectonic-typesetting/tectonic/tree/master/dist),
  with cross-platform Rust builds and complex deployment
- [WorldWideTelescope/wwt-webgl-engine](https://github.com/WorldWideTelescope/wwt-webgl-engine/tree/master/ci),
  with an NPM monorepo structure

## General structure

For many projects, it works well to adopt an overall pipeline structure with two
[stages][ap-stages]:

```yaml
trigger:
  branches:
    include:
    - master
    - rc

stages:
- stage: BuildAndTest
  jobs:
  - template: azure-build-and-test.yml

- stage: Deploy
  condition: and(succeeded('BuildAndTest'), ne(variables['build.reason'], 'PullRequest'))
  jobs:
  - template: azure-deployment.yml
```

The `BuildAndTest` stage can contain many parallel jobs that might build your
project on, say, Linux, MacOS, and Windows platforms. If all of those jobs
succeed, and the build is *not* a pull request (so, it was triggered in an
update to the `master` or `rc` branch), the deployment stage will run.

Here, we use [templates][ap-templates] to group the jobs for the two stages into
their own files. Templates are generally helpful for breaking CI/CD tasks into
more manageable chunks. However, they can be a bit tricky to get the hang of; a
key restriction is that templates are processed at “compile time”, and some
variables or other build settings are not known until “run time”.


## Installing Cranko

To install the latest version of Cranko into your build workers, we recommend
the following pair of tasks. By using a `condition` here, these tasks can be run
on [agents][ap-agents] running any operating system, and the right thing will
happen. This is useful if this setup step goes into a [template][ap-templates].

[ap-agents]: https://docs.microsoft.com/en-us/azure/devops/pipelines/agents/agents?view=azure-devops&tabs=browser
[ap-templates]: https://docs.microsoft.com/en-us/azure/devops/pipelines/process/templates?view=azure-devops

```yaml
- bash: |
    set -euo pipefail  # note: `set -x` breaks ##vso echoes
    d="$(mktemp -d /tmp/cranko.XXXXXX)"
    cd "$d"
    curl --proto '=https' --tlsv1.2 -sSf https://pkgw.github.io/cranko/fetch-latest.sh | sh
    echo "##vso[task.prependpath]$d"
  displayName: Install latest Cranko (not Windows)
  condition: and(succeeded(), ne(variables['Agent.OS'], 'Windows_NT'))

- pwsh: |
    $d = Join-Path $Env:Temp cranko-$(New-Guid)
    [void][System.IO.Directory]::CreateDirectory($d)
    cd $d
    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
    iex ((New-Object System.Net.WebClient).DownloadString('https://pkgw.github.io/cranko/fetch-latest.ps1'))
    echo "##vso[task.prependpath]$d"
  displayName: Install latest Cranko (Windows)
  condition: and(succeeded(), eq(variables['Agent.OS'], 'Windows_NT'))
```

If all of your agents will be running on the same operating system, you can
choose the appopriate task and remove the `condition`.


## Creating and transferring the release commit

If you use a multi-stage build process, a wrinkle emerges. You need to create a
single “release commit” to be published if the CI/CD succeeds. But if
publication happens in your `Deploy` stage, those jobs are separate from the
build jobs that actually ran the [`cranko release-workflow
apply-versions`](../commands/release-workflow-apply-versions.md) and [`cranko
release-workflow commit`](../commands/release-workflow-commit.md) commands.

We recommend publishing *the release commit* as an Azure Pipelines artifact.
This can be accomplished pretty conveniently with the [Git bundle][git-bundle]
functionality. *All* of your main build jobs should apply version numbers:

[git-bundle]: https://git-scm.com/book/en/v2/Git-Tools-Bundling

```yaml
- bash: |
    set -xeuo pipefail
    git status # [see below]
    cranko release-workflow apply-versions
  displayName: Apply versions with Cranko
```

(The `git status` helps on Windows, where it seems that sometimes `libgit2`
thinks that the working tree is dirty even though it’s not. There’s some issue
about updating the Git index file.)

*One* of your build jobs should also commit the version numbers into a release
commit, and publish the resulting commit as a Git bundle artifact:

```yaml
- bash: |
    set -xeuo pipefail
    git add .
    cranko release-workflow commit
    git show  # useful diagnostic
  displayName: Generate release commit

- bash: |
    set -xeuo pipefail
    mkdir $(Build.ArtifactStagingDirectory)/git-release
    git bundle create $(Build.ArtifactStagingDirectory)/git-release/release.bundle origin/master..HEAD
  displayName: Bundle release commit

- task: PublishPipelineArtifact@1
  displayName: Publish git bundle artifact
  inputs:
    targetPath: '$(Build.ArtifactStagingDirectory)/git-release'
    artifactName: git-release
```

(As a side note, if you run `bash` tasks on Windows, there is currently a bug
where variables such as `$(Build.ArtifactStagingDirectory)` are expanded as
Windows-style paths, e.g. `C:\foo\bar`, rather than Unix-style paths,
`/c/foo/bar`. You will either need to transform these variables, or not use bash
in Windows.)

Your deployment stages should then retrieve this artifact and apply the release
commit:

```yaml
# Fetch artifacts from previous stages
- download: current

# Check out source repo again
- checkout: self
  submodules: recursive

- bash: |
    set -xeuo pipefail
    git switch -c release
    git pull --ff-only $(Pipeline.Workspace)/git-release/release.bundle
  displayName: Restore release commit
```


# Standard deployment jobs

If your pipeline is running in response to an update to the `rc` branch, and
your CI tests succeeded, there are several common deployment steps that you can
invoke as (more or less) independent jobs. We recommend using a
[template][ap-templates] with standard setup steps to install Cranko and recover
the release commit, as shown above. Here we’ll assume that these have been
bundled into a template named `azure-deployment-setup.yml`.

We also assume here that you have a [variable group][ap-vargroups] called
`Deployment Credentials` that includes necessary credentials in variables named
`GITHUB_TOKEN`, `NPM_TOKEN`, etc.

[ap-vargroups]: https://docs.microsoft.com/en-us/azure/devops/pipelines/library/variable-groups?view=azure-devops

No matter which packaging system(s) you use, you should create tags and update
the upstream `release` branch. This example assumes that it lives on GitHub:

```yaml
- ${{ if eq(variables['Build.SourceBranchName'], 'rc') }}:
  - job: branch_and_tag
    pool:
      vmImage: ubuntu-latest
    variables:
    - group: Deployment Credentials
    steps:
    - template: azure-deployment-setup.yml

    - bash: |
        cranko github install-credential-helper
      displayName: Set up Git pushes
      env:
        GITHUB_TOKEN: $(GITHUB_TOKEN)

    - bash: |
        set -xeou pipefail
        cranko release-workflow tag
        git push --tags origin release:release
      displayName: Tag and push
      env:
        GITHUB_TOKEN: $(GITHUB_TOKEN)
```

### GitHub releases

If you are indeed using GitHub, Cranko can automatically create [GitHub
releases][gh-releases] for you. You must ensure that this task runs *after* the
tags are pushed, because otherwise GitHub will auto-create incorrect tags for
you:

[gh-releases]: https://docs.github.com/en/github/administering-a-repository/about-releases

```yaml
- ${{ if eq(variables['Build.SourceBranchName'], 'rc') }}:
  - job: github_releases
    dependsOn: branch_and_tag # otherwise, GitHub creates the tags itself!
    pool:
      vmImage: ubuntu-latest
    variables:
    - group: Deployment Credentials

    steps:
    - template: azure-deployment-setup.yml

    - bash: cranko github install-credential-helper
      displayName: Set up Git pushes
      env:
        GITHUB_TOKEN: $(GITHUB_TOKEN)

    - bash: cranko github create-releases
      displayName: Create GitHub releases
      env:
        GITHUB_TOKEN: $(GITHUB_TOKEN)
```

You might also use [`cranko github
upload-artifacts`](../commands/cicd/github-upload-artifacts.md) to upload
artifacts associated with those releases, although if you have a monorepo you
must use [`cranko show if-released`](../commands/util/show.md) to check at
runtime whether the project in question was actually released.

### Cargo publication

If your repository contains Cargo packages, you should publish them:

```yaml
- ${{ if eq(variables['Build.SourceBranchName'], 'rc') }}:
  - job: cargo_publish
    pool:
      vmImage: ubuntu-latest
    variables:
    - group: Deployment Credentials

    steps:
    - template: azure-deployment-setup.yml

    - bash: cranko cargo foreach-released publish
      displayName: Publish updated Cargo crates
      env:
        CARGO_REGISTRY_TOKEN: $(CARGO_REGISTRY_TOKEN)
```

### NPM publication

Likewise for NPM packages:

```yaml
- ${{ if eq(variables['Build.SourceBranchName'], 'rc') }}:
  - job: npm_publish
    pool:
      vmImage: ubuntu-latest
    variables:
    - group: Deployment Credentials

    steps:
    - template: azure-deployment-setup.yml

    - bash: cranko npm install-token
      displayName: Set up NPM authentication
      env:
        NPM_TOKEN: $(NPM_TOKEN)

    # [ do any necessary build stuff here ]

    - bash: cranko npm foreach-released npm publish
      displayName: Publish to NPM

    - bash: shred ~/.npmrc
      displayName: Clean up credentials
```
