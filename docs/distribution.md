# Installation and distribution

## Download and run

1. Open a successful [build run](https://github.com/Tuim-editor/Gtuim/actions/workflows/build-release.yml).
2. Download the artifact for your platform (GitHub sign-in required).
3. Unzip the artifact, extract its `.tar.gz`, and open the extracted folder.
4. Run `./gtuim`, or `./gtuim path/to/project`.

| Platform | Artifact |
| --- | --- |
| Linux x86-64 | `gtuim-x86_64-unknown-linux-gnu` |
| macOS Intel | `gtuim-x86_64-apple-darwin` |
| macOS Apple Silicon | `gtuim-aarch64-apple-darwin` |

If using the raw executable instead, restore permissions with `chmod +x filename`.
macOS binaries are unsigned and not notarized; Gatekeeper may require approval
through the system security UI. These are executables, not `.app` installers.

## Requirements

Keep these tools on the launching terminal's `PATH`:

- **Neovim 0.10+** and **Git** for editing and plugin setup.
- **ripgrep** for project search; **Python 3** for the plugin marketplace.
- **Poppler** (`pdfinfo` and `pdftoppm`) for PDF preview.
- Compilers and language servers for the languages you use, as needed.

The first launch opens Welcome & Setup. Completing it remembers Gtuim setup
across launches and profiles. **Skip for now** defers it until the next launch.
Reopen it later through **Help → Welcome & Setup**.
Plugin setup may access GitHub. Language tools are installed separately.

Linux needs a graphical session, working GPU/Vulkan support, glibc, XCB and
xkbcommon. Inspect a binary's libraries with `ldd ./gtuim`. CI Linux packages use
Ubuntu 24.04 as a baseline; binaries built on newer distributions may not run on
older systems.

## Build from source

Install Rust. On Ubuntu 24.04, install the native build libraries:

```sh
sudo apt-get update
sudo apt-get install build-essential clang cmake pkg-config libasound2-dev \
  libfontconfig1-dev libfreetype6-dev libwayland-dev libx11-xcb-dev libxcb1-dev \
  libxkbcommon-x11-dev libvulkan-dev libssl-dev
```

On macOS, install full Xcode and its Metal tools. Check that `xcrun --find clang`
and `xcrun --find metal` succeed. GPUI requires these Apple tools; adding a macOS
Rust target to a Linux machine is not enough.

```sh
git clone https://github.com/Tuim-editor/Gtuim.git
cd Gtuim
cargo build --locked --release
./target/release/gtuim
```

## Package a build

Build and package on each native platform:

```sh
# Linux example; use x86_64-apple-darwin or aarch64-apple-darwin on a Mac.
cargo build --locked --release --target x86_64-unknown-linux-gnu
bash scripts/package-release.sh x86_64-unknown-linux-gnu
```

Commit source changes first; packaging rejects a dirty source tree. Output is
written to `dist/`: a raw executable, `.tar.gz` package and `.sha256` checksums.
The package includes the GPL license, upstream notices, project source snapshot,
source revision and these instructions. Share the archive so recipients receive
those files with the executable. Dependencies retain their own licenses; pinned
versions and checksums are in `Cargo.lock`.

For all three platforms, run **Actions → Build shareable executables → Run
workflow**, or push a `v*` tag. CI builds, tests and uploads artifacts; it does not
publish a GitHub Release or sign/notarize the binaries. CLI and headless tests do
not replace interactive GUI testing on each operating system.
