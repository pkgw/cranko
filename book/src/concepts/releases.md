# Releases

Cranko’s idea of a *release* closely tracks the one implied by the [semantic
versioning specification][semver]. Each [project](./projects.md) in a repo is
sent out into the world in a time-series of releases. Each release is associated
with a [version](./versions.md), a Git commit, and some set of *artifacts*,
which are almost always “files” in the standard computing sense. All of these
should be immutable and, to some extent, irrevocable: once you’ve made a
release, it’s never coming back. This sounds dangerous, but a big point of
release automation is to make the release process so easy that if you mess
something up, it’s easy to supersede it with a fixed version.

[semver]: https://semver.org/

The fundamental assumption of Cranko is that we are seeking to achieve total
automation of the software release process. It is important to point out that
Cranko does not seek to automate the *decision to release*: it is the authors’
opinion that it is important for this decision to be in human hands. (Although
if you want to automate that decision too, we can’t and won’t stop you.) But
once that decision has been made, as much of the process involved should proceed
mechanically. We believe that the [just-in-time versioning][jitv] workflow
provides an extremely sound basis on which to make this happen.

[jitv]: ../jit-versioning/index.md

Because repositories can contain multiple projects, an individual commit in a
repository’s history might be associated with zero, one, or *many* project
releases. This model requires a certain amount of trust: if I release project X
in commit Y, I’m implictly asserting that all projects not-X do *not* need to be
released at the same time. There is no way for a computer to know that this is
actually true. (The same kind of trust is required by Git’s merge algorithm,
which assumes that if two different commits do not alter the same part of the
same files, that they do not conflict with one another. This assumption is a
good heuristic, but not infallible.) In Cranko’s case, the only way to avoid
placing this trust in the user would be to demand that the release of *any*
project requires the release of *all* projects, which is takes cautiousness to
the level of absurdity.
