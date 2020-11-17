# Integrations: Python

Cranko supports Python projects set up using [PyPA]-compliant tooling. Because
the Python packaging ecosystem contains a lot of variation, Cranko often needs
you to give it a few hints to be able to operate correctly.

[PyPA]: https://www.pypa.io/


## Autodetection

Cranko identifies Python projects by looking for directories containing files
named `setup.py`, `setup.cfg`, *or* `pyproject.toml`. It is OK if one directory
contains more than one of these files.


## Project Metadata

While the Python packaging ecosystem is moving towards standardized metadata
files, there are still lots of projects where the package name and version are
specified only in the `setup.py` file. The only fully correct way to extract
these metadata would be to execute arbitrary Python code, which isn’t possible
for Cranko. Instead, Cranko uses a variety of more superficial techniques to try
extract project metadata.

### Project name

1. If there is a `pyproject.toml` file containing a key `name` in a
   `tool.cranko` section, that value is used as the project name.
1. Otherwise, if there is a `setup.cfg` file containing a `name` key in a
   `metadata` section, that value is used as the project name.
1. Otherwise, there should be a `setup.py` file containing a line with the
   following form:
   ```python
   project_name = "myproject"  # cranko project-name
   ```
   Specifically, Cranko will search for a line containing a comment with the
   text `cranko project-name`. Within such a line, it will then search for a
   string literal and extract its text as the project name. Cranko’s parsing of
   Python string literals is quite naive — escaped characters and the like won’t
   work.

### Project version

Cranko will extract the project version from either `setup.py`, or from an
arbitrary other Python file (i.e., from `myproject/version.py` or something
similar). To tell Cranko to search for the version from a file *other* than
`setup.py`, ensure that your project has a `pyproject.toml` file and add an
entry of this form:

```python
[tool.cranko]
main_version_file = "myproject/version.py"
```

The path should be relative to the directory containing the `pyproject.toml`
file.

Within that file, there are two options:

1. If your project’s version is expressed as [sys.version_info] tuple, annotate
   it with a comment containing the text `cranko project-version tuple`:
   ```python
   version_info = (1, 2, 0, 'final', 0)  # cranko project-version tuple
   ```
   Cranko will parse the tuple contents into a [PEP-440] version and rewrite it
   as needed. Note that some PEP-440 versions are not expressible as
   [sys.version_info] tuples. Also, Cranko’s tuple parser is quite naive, and
   only handles the most basic form of Python’s tuple, integer, and string
   literals. When your repo is bootstrapped, this line will be rewritten to look
   like:
   ```python
   version_info = (0, 0, 0, 'dev', 0)  # cranko project-version tuple
   ```
   because Cranko will start managing the version number.
1. If your project’s version is expressed as a string literal, annotate
   it with a comment containing just the text `cranko project-version`:
   ```python
   version = '1.2.0'  # cranko project-version
   ```
   Cranko will search for a string literal in the line and parse it as a
   [PEP-440] version. Here too, Cranko’s parsing of the literal is quite naive
   and only handles the most basic forms. When your repo is bootstrapped, this
   line will be rewritten to look like:
   ```python
   version = '0.0.0.dev0'  # cranko project-version tuple
   ```
   because Cranko will start managing the version number.

[sys.version_info]: https://docs.python.org/3/library/sys.html#sys.version_info
[PEP-440]: https://www.python.org/dev/peps/pep-0440/


## Additional Annotated Files

If there are files within your Python project besides `setup.py` or your
`main_version_file` that can provide useful metadata to Cranko — or will need
rewriting by Cranko to update versioning and/or dependency information — you
must tell Cranko which files it should check. Otherwise, Cranko would have to
scan every file in your repository, which would significantly slow it down with
large projects.

Tell Cranko which additional files to search by adding an `annotated_files` key
to a `tool.cranko` section in a `pyproject.toml` file for your project:

```toml
[tool.cranko]
annotated_files = [
  "myproject/npmdep.py",
  "myproject/rustdep.py",
]
```


## Internal Dependencies

“Internal” dependencies refer to [monorepo] situations where one repository
contains more than one project, and some of those projects depend on one
another.

[monorepo]: https://en.wikipedia.org/wiki/Monorepo

Cranko actually doesn’t yet automatically recognize internal dependencies
between multiple Python projects within one repository — the monorepo model
seems to be extremely rare for Python packages. It does, however, recognize
internal dependencies in a generic fashion that is useful if, for instance, your
repo contains a [JupyterLab extension] that consists of a Python package that is
tightly coupled to an NPM package.

[JupyterLab extension]: https://jupyterlab.readthedocs.io/en/stable/user/extensions.html

Internal dependencies can be marked by tagging the dependency version
requirement in one of your [annotated files]. Ensure that one or more of these
files contains a line of code with the following form:

```python
npm_requirement = '1.2.0'  # cranko internal-req myfrontend
```

In this example, the python package has a dependency on the project name
`myfrontend`, and that it requires version 1.2.0 or greater. (Here we envision
that the `myfrontend` project is an NPM package, so that this version
requirement is a [semver] requirement.) As with other annotations, all that
Cranko does here is to search for something that looks like a string literal
within the tagged line, and attempt to parse it. As far as Cranko is concerned,
the only thing that matters in the annotated line is what happens inside the
string literal delimeters. You don’t need to do anything with the associated
variable (`npm_requirement`), or even assign the string literal to a variable,
if it’s not needed in your code.

When you bootstrap your project, your tagged line will be rewritten to resemble
something like:

[annotated files]: #additional-annotated-files
[semver]: https://semver.org/

```python
npm_requirement = '0.0.0-dev.0'  # cranko internal-req myfrontend
```

because Cranko takes over the expression of concrete version requirements in the
repo.

### Versioning internal dependencies

As described in [Just-in-Time Versioning][jitv-int-deps], Cranko needs the
version requirements of internal dependencies to be expressed as *Git commits*
rather than version numbers. These requirements must be expressed in the
`pyproject.toml` file using the following structuring:

```toml
[tool.cranko.internal_dep_versions]
"myfrontend" = "2937e376b962162067135f3ac8b7b6a0f1c3efea"
```

This entry expresses that the Python project requires a release of the
`myfrontend` package that contains the Git commit `2937e376...`. When Cranko
rewrites your project files during release processing, it will translate this
requirement into a concrete version number and update your [annotated files]
with the appropriate expression.

[jitv-int-deps]: ../jit-versioning/#the-monorepo-wrinkle

**TODO**: write some generic docs about these requirement expressions, and link
to them from here.