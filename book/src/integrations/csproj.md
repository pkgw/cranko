# Integrations: Visual Studio C# Projects

Cranko has basic support for managing Visual Studio C# projects, based on
`AssemblyInfo.cs` files. This support has been developed for a narrow use-case
and could potentially become much more sophisticated.


## Autodetection

Cranko identifies C# projects by looking for directories that contain a file
with a name ending in `.csproj` *and* another file with a name matching the
pattern `*/AssemblyInfo.cs`. Cranko will get confused if you have more than one
`.csproj` file in a single directory.

Cranko additionally searches for "setup installer" project files, whose names
end in `.vdproj`. If such a file is found, *and* it seems to refer to a single
"primary output project" recognized by Cranko (via a `OutputProjectGuid` key),
the `ProductVersion` key in the file will be updated to track the corresponding
project version.


## Project Metadata

Project metadata are extracted in a fairly basic manner:

### Project name

The project name is taken to be the contents of the last `<AssemblyName>`
element in the `.csproj` XML file.

### Project version

Cranko will extract the project version from the `AssemblyVersion` attribute of
a project's `AssemblyInfo.cs` file. In particular, it searches for a line
starting with the exact text `[assembly: AssemblyVersion`, and extracts whatever
is between double quotation marks on that line.

C# project versions emulate the [.NET
System.Version](../concepts/versions.md#net-versions) type.

When updating project files, both the `AssemblyVersion` and the
`AssemblyFileVersion` attributes are updated, if present.

If a project has one or more associated `.vdproj` installer projects, the
`ProductVersion` stored with the installer(s) will lose the fourth component
(the "revision") of the project version, because four-component versions are
rejected by the installer builder. The `PackageCode` and `ProductCode` of the
installer will be replaced with a new, randomly-generated UUID (the same one for
both codes). This is a conservative, and possibly sketchy, approach, since it
means that different installer versions will unconditionally be treated as
["major upgrades"]. See [Changing the Product Code][ctpc] for more information.

["major upgrades"]: https://docs.microsoft.com/en-us/windows/win32/msi/major-upgrades
[ctpc]: https://docs.microsoft.com/en-us/windows/win32/msi/changing-the-product-code


## Internal Dependencies

[“Internal” dependencies](../concepts/internal-dependencies.md) refer to
[monorepo] situations where one repository contains more than one project, and
some of those projects depend on one another.

[monorepo]: https://en.wikipedia.org/wiki/Monorepo

Cranko automatically detects internal dependencies between C# projects by
searching for `<Project>` elements in the `.csproj` XML file, where the text
contents of these elements give the GUID of another project. Such elements
should be contained inside a `<ProjectReference>` element but Cranko's parser
doesn't bother to require that.

As described in [Just-in-Time Versioning][jitv-int-deps], Cranko operates under
a model where every internal dependency should be associated with a minimum
compatible version of the dependee project, expressed as a *Git commit* rather
than a version number.

[jitv-int-deps]: ../jit-versioning/index.md#the-monorepo-wrinkle

There is (currently?) no place where Cranko outputs internal dependency version
requirements into the project files, because such requirements are automatically
embedded into C# assemblies at build time by the compiler. However, Cranko still
prompts you to annotate your projects with this information, because it can help
you keep track of when new project releases must be made. These requirements
should be recorded in each project's `.csproj` file in the following way:


```xml
<ProjectExtensions>
   <Cranko>
      <CrankoInternalDepVersion>{c05266fe-6947-42f1-9863-7cdbeed60869}=manual:unused</CrankoInternalDepVersion>
      <CrankoInternalDepVersion>{GUID}={req}</CrankoInternalDepVersion>
   </Cranko>
</ProjectExtensions>
```

Each `<CrankoInternalDepVersion>` item associates a dependency, identified by
its GUID, with a version requirement. You can use `manual:unused` if you don't
want to track such information in detail.
