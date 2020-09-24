# Versions

Every [project](./projects.md) in Cranko has one or more
[releases](./releases.md), each of which is associated with a version (AKA
“version number”). We can think of the version of the most recent release as
being the “current” version of the project, but it is important to remember that
in a given repository a project may be in an intermediate state between releases
and hence between well-defined version numbers.

Cranko’s model takes pains to avoid strong assumptions about what version
“numbers” look like — they don't even need to be numbers — or how they change
over time. Well-specified versioning syntaxes like [semver][semver2] are
supported, but the goal is to make it possible to use domain-specific syntaxes
as well. In particular, at the moment, Cranko only supports one scheme:

- [Semantic Versioning (“semver”) versions](#semantic-versioning-versions)

## Semantic Versioning versions

“Semver” versions follow the [Semantic Versioning 2][semver2] specification.
They generally follow a `MAJOR.MINOR.MICRO` structure with optional extra
prerelease and build metadata. The semver specification is rigorously defined
(as you’d hope), so consult that document for details.

[semver2]: https://semver.org/

Used by Cargo and NPM packages.
