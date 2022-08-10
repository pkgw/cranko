# Integrations: Zenodo

Cranko supports safe, automatic software [DOI] registration through the [Zenodo]
service operated by [CERN] in collaboration with other scientific organizations.

[DOI]: https://www.doi.org/
[Zenodo]: https://zenodo.org/
[CERN]: https://home.cern/


## Orientation: Software DOIs

While most people think of DOIs as associated with scholarly publications, more
and more DOIs are being associated with other forms of digital academic output.
And, of course, software is more and more becoming an important form of digital
academic output! While it is beyond the scope of this documentation to explain
software DOIs in depth, it is worth mentioning the distinction between a
*version DOI* and a *concept DOI*.

Version DOIs are perhaps more familiar. Just like each release of a software
package is assigned a unique version number, each release of a software package
can be assigned a unique DOI corresponding to that version. If you want to know
which specific version of a piece of software that someone was using, either
the exact version number or the exact version DOI will tell you that.

If all you care about is knowing what software someone was running, then version
DOIs don't add anything new that version numbers don't already provide. However,
unlike version numbers, DOIs are first-class items in the scholarly publishing
information ecosystem. When you give software a DOI, it can be integrated into
that ecosystem in way that isn't possible otherwise. Probably the most important
aspect of this is that software DOIs can be associated with author lists and
[ORCID iDs](https://orcid.org/) using standard scholarly metadata systems, so
that researchers can get personal credit when their software is used and cited!

Because we want to be able to know exactly what piece of code a person was
running, we absolutely want to create a new DOI for each release of a software
package. But if that package has a whole bunch of releases, we have a whole
bunch of different DOIs, which is going to make it really tedious to quantify
the usage of the package overall. This is where concept DOIs come in. Concept
DOIs don’t really carry any information on their own, but they can be used in
the DOI metadata framework to link together different releases of the same
software package in a machine-understandable way. While the DOI
[10.5281/zenodo.6963051] is a machine-usable way to talk about “version 4.21.1
of the [transformers]” package, the concept DOI [10.5281/zenodo.3385997] is a
machine-usable way to refer to the thing that is “the transformers package”
overall.

[10.5281/zenodo.6963051]: https://doi.org/10.5281/zenodo.6963051
[transformers]: https://huggingface.co/transformers
[10.5281/zenodo.3385997]: https://doi.org/10.5281/zenodo.3385997


## Workflow Overview

Cranko’s support for Zenodo “deposition” involves a multi-stage process. It
follows the principles of the [just-in-time versioning approach][jitv] where
release metadata only ever appear in tested release artifacts.

[jitv]: ../jit-versioning/index.md

- During the beginning of CI/CD processing, a new Zenodo deposit is
  [preregistered][prereg], and the DOIs that *will be* created are obtained.
  These can be inserted into the source files of your software, so that it can
  print out its own DOI. This step can be run during pull-request processing:
  but instead of doing anything with the Zenodo API, fake DOIs are generated and
  used.
- Once CI/CD tests have all passed, you can [upload artifacts][upload] if so
  desired, then actually [publish] the release. Zenodo will actually register
  the DOIs.
- Because Zenodo deposits are associated with version numbers, each deposit
  process is associated with a specific cranko [project]. In a monorepo
  scenario, you can run multiple deposits for multiple projects as you see fit.

[prereg]: ../commands/cicd/zenodo-preregister.md
[upload]: ../commands/cicd/zenodo-upload-artifacts.md
[publish]: ../commands/cicd/zenodo-publish.md
[project]: ../concepts/projects.md

## Getting Started

To start using the Zenodo integration, you need to create a [Zenodo metadata
file][zmeta] somewhere in your repository. This file is traditionally called
`zenodo.json5` and can be stored anywhere you feel like.

[zmeta]: ../configuration/zenodo.md

While you should see the [Zenodo Metadata Files][zmeta] page for the full
details of the file format, the short version is that it has two main fields.
The first, `"metadata"`, contains the metadata that will describe your Zenodo
deposition. See [the Zenodo developer documentation][mdformat] for a precise
definition of all of the fields that can be used here. *The contents here are
things you need to decide for yourself,* including, most importantly, the author
list that you want to associate with your project.

[mdformat]: https://developers.zenodo.org/#deposit-metadata

The second field, `"conceptrecid"`, will be used to ensure that successive
releases of your project are all tied together with the same concept DOI. When
creating the first Zenodo release of your software, you should set this to the
special value `"new-for:$version"`, where `$version` is the planned next version
of the project being released. For instance, you might put `"new-for:0.12.0"` at
first. If the preregistration process runs for a *different* version, it will
error out. This precaution helps make sure that you don’t forget to update your
metadata file once the concept DOI has been created.

If you're using a monorepo, you can make as many Zenodo releases as you like
during CI processing. Just run the relevant commands as many times as needed,
and create a different Zenodo configuration file for each project that gets
assigned DOIs.


## Rewrites

The ['cranko zenodo preregister`][prereg] command can insert the DOIs that *will
be* registered into your source code. You can use this functionality to create
software releases that *know their own DOIs*.

We suggest that you include commands in your software to print out these DOIs,
along the lines of [`cranko show cranko-version-doi`] and [`cranko show
cranko-concept-doi`]. This way, there is an easy way for users to get the
precise DOIs relating to the software that they're running. You might also want
to insert these DOIs into logs or metadata associated with the files that your
software creates, although in many cases the version number is going to be more
understandable to users.

[`cranko show cranko-version-doi`]: ../commands/util/show.md#cranko-show-cranko-version-doi
[`cranko show cranko-concept-doi`]: ../commands/util/show.md#cranko-show-cranko-concept-doi

This insertion happens during the ['cranko zenodo preregister`][prereg] command,
which will rewrite any files whose paths you pass to it on the command line.
The following rewrite rules are followed:

- The text `xx.xxxx/dev-build.$project.concept`, where `$project` is the name of
  the Cranko project being released, is replaced with the concept DOI.
- The text `xx.xxxx/dev-build.$project.version`, where `$project` is the name of
  the Cranko project being released, is replaced with the version DOI.

If you’re building out of source control, these replacements won't happen, of
course. If a pull request is being built, fake DOIs with similar forms will be
substituted in. You can add checks in your code to see whether the DOIs start
with the universal DOI prefix, `"10."`, to know whether your DOIs are real or
fake.


## CI/CD Workflow

Zenodo publication operations require you to have a [Zenodo API token][zdev],
which you can create in the [Zenodo Account Tokens page][ztok]. You need to get
this token into the environment variable `ZENODO_TOKEN` for the Zenodo workflow
to work.

[zdev]: https://developers.zenodo.org/
[ztok]: https://zenodo.org/account/settings/applications/tokens/new/

The ['cranko zenodo preregister`][prereg] command(s) should be run at the
beginning of your CI/CD workflow, before [`cranko release-workflow commit`].
After it runs, you should `git add` your modified files to make sure they get
included in the release commit.

[`cranko release-workflow commit`]: ./release-workflow-commit.md

At the end of your CI/CD workflow, if you are actually making real releases, you
should run ['cranko zenodo upload-artifacts`][upload] as needed, then finally
['cranko zenodo publish`][publish] to publish your new deposits.


## Continued Releases

After your first successful Zenodo deposit, you should update your
`zenodo.json5` file and replace the special `"conceptrecid"` field with the
Zenodo record ID corresponding to the “concept” of your software package. This
is easily findable in the concept DOI, and is also printed by
['cranko zenodo preregister`][prereg].

Going forward, you should review the `zenodo.json5` file periodically and update
as needed — in particular, you should be attentive to the author list. As with
any academic product, the choice of who goes on an author list, and what order
that list is in, is not something that can be automated — you have to decide how
you want to handle it.
