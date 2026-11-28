# Gtuim

A desktop code editor powered by **Neovim** and **GPUI**, for Linux and macOS.

## Features

- **Editing:** tabs, split views, line numbers, completion, and Vim or familiar controls.
- **Files and search:** project explorer, quick file opening, search and replace.
- **Git:** initialize repositories, stage changes, preview staged and working-tree diffs, commit, switch local or remote branches, browse history, and manage worktrees.
- **Terminals:** run commands without leaving the editor.
- **Build tools:** compile your project and jump to errors.
- **Previews:** read Markdown (including formatted tables) and PDFs beside your code, and view PNG/JPEG images natively.
- **Customization:** themes, fonts, shortcuts, plugins, and language tools.
- **Profiles:** separate settings, activity tracking, and optional coding practice.
- **AI tools:** launch supported coding assistants installed on your machine.

## Get started

Download your platform's artifact from the [builds](https://github.com/Tuim-editor/Gtuim/actions/workflows/build-release.yml) (GitHub sign-in required). Unzip it, extract the included `.tar.gz`, and open the extracted folder:

```sh
./gtuim
# Or open a file or project:
./gtuim path/to/project
```

Install **Neovim 0.10+** and **Git** on your `PATH`. Project search uses **ripgrep**; the plugin marketplace uses **Python 3**. PDF preview needs **Poppler**.

The welcome screen opens on your first launch to set up your theme and controls. Use **F1** for commands, **Ctrl+S** to save, and **Ctrl+Q** to quit (**Cmd** on macOS).

See [platform requirements and installation notes](docs/distribution.md). macOS packages are unsigned and may require system approval.

## Build from source

Install Rust and the [build dependencies](docs/distribution.md#build-from-source) (Linux development libraries or macOS Xcode/Metal), then:

```sh
git clone https://github.com/Tuim-editor/Gtuim.git
cd Gtuim
cargo build --locked --release
./target/release/gtuim
```

## Preview and Git controls

- Open a `.png`, `.jpg`, or `.jpeg` file to preview it automatically. **Ctrl+Shift+I** or **View → Toggle Image Preview** switches between the preview and raw buffer; the preview toolbar offers full/split view, fit, zoom, and background controls.
- **Ctrl+Shift+M** toggles live Markdown preview, including tables and inline formatting.
- In **Source Control**, use **Initialize Repository** for a project without Git, click the branch header to switch branches, or click a changed file to inspect its staged or working-tree diff. Branch switching preserves local edits and reports conflicts instead of forcing a checkout.

## Help and feedback

- [Plugins](docs/plugins.md) · [Profiles](docs/profiles.md) · [Coding practice](docs/practice.md)
- Press **F12** to prepare a bug report, then review and submit it on [GitHub](https://github.com/Tuim-editor/Gtuim/issues).

## License

[GPL-3.0-or-later](LICENSE). Based on [Rtuim](https://github.com/Tuim-editor/Rtuim), with its [upstream MIT notice](LICENSES/Rtuim-MIT.txt) preserved.
