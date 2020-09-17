# Configuration

Cranko aims to “just work” with minimal explicit configuration. That being said,
flexibility is clearly important in a workflow tool. If some aspect of Cranko’s
behavior isn’t configurable, the reason is probably simply that no one has
gotten around to wiring up the necessary code, rather than a reluctance to allow
flexibility.


## The per-repository configuration file

For each Cranko-using repository, the main configuration file is located at
`.config/cranko/config.toml`. Cranko can run without this file, and the hope is
that the tool can be very useful without requiring the file’s presence.

For reproducibility and testability, the goal is that as much Cranko
configuration as possible can be centralized in this file, without per-user or
per-environment customizations. At the moment, no other Cranko configuration
files are supported.

The `config.toml` file may contain the following items:

- [`[repo]`](#the-repo-section) — Configuration relating to the backing repository
  - [`upstream_urls`](#the-uptream_urls-field) — How the upstream remote is recognized

As mentioned above, additional items are planned to be added as the need arises.

### The `[repo]` section

This section contains configuration relating to the backing Git repository.

### The `upstream_urls` field

This field is a list of strings giving the Git URLs associated with the
canonical upstream repository, which is the one that will perform automated
release processing upon updates to its `rc`-like branch. For example:

```
upstream_urls = [
  "git@github.com:pkgw/cranko.git",
  "https://github.com/pkgw/cranko.git"
]
```

(The *name* of the upstream remote might change from one checkout to the next,
but the set of canonical upsteam *URLs* should be small.)

The ordering of the URLs does not matter. If the list is empty (i.e. it is
unspecified), and there is only one remote, Cranko will use it. If there is more
than one remote but one is named `origin`, Cranko will use that. Otherwise,
Cranko will error out. If more than one remote matches any of the URLs, one of
them will be used but it is unspecified which.
