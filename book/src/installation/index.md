# Installation

Because Cranko is delivered as a single standalone executable, it is easy to install.
This is very intentional!

There are several installation options:

- On a Unix-like operating system (Linux or macOS), the following command will
  place the latest release of the `cranko` executable into the current directory:
  ```shell
  curl --proto '=https' --tlsv1.2 -sSf https://pkgw.github.io/cranko/fetch-latest.sh | sh
  ```
  If your CI/CD environment doesn't make Cranko available in a more standardized
  way, this is the recommended installation command.
- On Windows, the following PowerShell commands will do the same:
  ```powershell
  [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager::SecurityProtocol -bor 3072
  iex ((New-Object System.Net.WebClient).DownloadString('https://pkgw.github.io/cranko/fetch-latest.ps1'))
  ```
- You can manually download precompiled binaries from the Cranko [GitHub release
  archive][github-releases].
- If you have a [Rust] toolchain installed, you can compile and install your own
  version with `cargo install cranko`.
- Finally, to develop Cranko itself, you can check out [the source code] and
  build using the standard Rust framework: `cargo build`.

[github-releases]: https://github.com/pkgw/cranko/releases/latest
[Rust]: https://www.rust-lang.org/tools/install
[the source code]: https://github.com/pkgw/cranko/

Note that, to fully implement the [just-in-time
versioning](../jit-versioning/index.md) workflow, the `cranko` command will need
to be available both on your development machine and on the nodes powering your
CI/CD pipeline. The `curl` and PowerShell commands given above should make
installation easy on just about any CI/CD system. The code for these installers
is almost directly ripped off from [Rustup] and [Chocolatey], respectively
(thanks!), and will honor some of the environment variables used by those
installers.

[Rustup]: https://github.com/rust-lang/rustup/blob/master/rustup-init.sh
[Chocolatey]: https://github.com/chocolatey/chocolatey.org/blob/master/chocolatey/Website/Install.ps1
