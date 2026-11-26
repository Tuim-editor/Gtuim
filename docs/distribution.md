# Sharing Gtuim

## Release files

Release packaging produces a directly executable `gtuim-<version>-<platform>` and a
`.tar.gz` containing `gtuim`, its GPL license, upstream notices, these instructions,
a `SOURCE_COMMIT` identifier and `gtuim-source.tar.gz` (the matching project's
committed source and build files). SHA-256 checksums cover both release files.
Gtuim is licensed under `GPL-3.0-or-later`; see `LICENSE` and the README license
section. Share the archive, not only a detached executable, so recipients also
receive the license, notices and project source. Dependencies retain their own
licenses; their pinned versions and checksums are recorded in `Cargo.lock` and
Cargo retrieves their sources when building. This is not a comprehensive audit
of third-party distribution obligations. Supported release targets:

| Platform | Rust target |
| --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple Silicon | `aarch64-apple-darwin` |

These are different native builds, not copies with different filenames. The
macOS executables require a native Mac build with Xcode and its Metal tools;
installing a Rust target on Linux alone is insufficient for GPUI.

## Running a downloaded executable

Extract the archive and run `./gtuim --version`, then `./gtuim [FILES...]`.
For the raw executable, use its platform-suffixed filename instead. Download
services may lose execute permissions; restore them with `chmod +x <filename>`.

Gtuim embeds its Lua runtime and artwork but is **not dependency-free**:

- A recent Neovim must be installed and on `PATH`. The runtime uses APIs from
  Neovim 0.10 or later; older releases are unsupported (this is an API floor, not
  a claim that every 0.10 configuration has been tested). Git is needed for plugin
  bootstrap and source-control operations; ripgrep enables project search.
- Python 3 is needed for Plugin Center/marketplace operations. Embedding the
  store-search script does not embed its interpreter.
- On first use, plugin bootstrap may access GitHub. Language tools and practice
  compilers are separate optional installations.
- PDF preview additionally needs Poppler (`pdfinfo` and `pdftoppm`).
- Linux needs a graphical session, GPU/Vulkan support and the system libraries
  linked by the executable (including XCB, xkbcommon and glibc). Use `ldd ./gtuim`
  to inspect them. A binary built on a rolling-release distro is not guaranteed
  to run on older distributions. CI builds use Ubuntu 24.04 as their baseline.
- macOS files are unsigned and not notarized. Gatekeeper may require explicit
  approval using the system's security UI. These are terminal-launched executables,
  not signed `.app` bundles or installers. Prefer an installed Neovim reachable
  from the launching terminal's `PATH`.

## Building locally

```sh
rustup target add x86_64-unknown-linux-gnu
cargo build --locked --release --target x86_64-unknown-linux-gnu
bash scripts/package-release.sh x86_64-unknown-linux-gnu
```

On a Mac, substitute its native target from the table. Output goes to `dist/`.
Commit the source before building and packaging; packaging refuses a dirty source
tree to avoid shipping an inaccurate source snapshot. The packaging script checks
`--version` before creating archives. That is a CLI
smoke test, not proof of rendering or editing functionality; run the GUI and
open/edit/save a file on each target OS before distributing widely.

## Native CI builds

`.github/workflows/build-release.yml` builds and uploads all three platforms on
native Linux, Intel Mac and Apple Silicon Mac runners. Once this project is
pushed to its agreed GitHub repository, run **Actions → Build shareable
executables → Run workflow**, or push a `v*` tag. Download each job's artifact
from the completed run. The workflow does not publish a GitHub Release, sign
binaries, or modify the issue tracker.

The source repository is now `https://github.com/Tuim-editor/Gtuim`, alongside
`Tuim-editor/Tuim` and `Tuim-editor/Rtuim`. Adding a workflow file does not itself
produce macOS executables: an actual successful native workflow run is required.
Private repository contents and CI artifacts require access; archive files can
be shared separately together with their source and notices. Runner availability and any private-repository CI charges are governed
by the repository's GitHub plan.
