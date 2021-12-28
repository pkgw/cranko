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
as well. In particular, at the moment, Cranko supports three schemes:

- [Python (“PEP440”) versions](#python-pep-440-versions)
- [Semantic Versioning (“semver”) versions](#semantic-versioning-versions)
- [.NET versions](#net-versions)


## Python PEP-440 versions

Python packages are assumed to be versioned according to [PEP-440]. This is a
very flexible scheme that allows any number of primary numbers as well as
“alpha”, “beta”, “rc”, “dev”, *and* “post” sequencing. Consult [PEP-440] for
details.

[PEP-440]: https://www.python.org/dev/peps/pep-0440/

Used by Python packages.


## Semantic Versioning versions

“Semver” versions follow the [Semantic Versioning 2][semver2] specification.
They generally follow a `MAJOR.MINOR.MICRO` structure with optional extra
prerelease and build metadata. The semver specification is rigorously defined
(as you’d hope), so consult that document for details.

[semver2]: https://semver.org/

Used by Cargo and NPM packages.


## .NET Versions

.NET versions emulate the .NET [System.Version][sysver] type. This is a simple
type following the form `MAJOR.MINOR.BUILD.REVISION`, where each piece is an
integer. The maximum allowed value of each item is 65534.

[sysver]: https://docs.microsoft.com/en-us/dotnet/api/system.version

Used by packages in [Visual Studio C# projects](../integrations/csproj.md).

The `micro bump` version bump syntax will update the "build" component of a
version string. There is currently no syntax to bump the revision component of a
version string.