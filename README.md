# Gtuim

A desktop code editor powered by **Neovim** and **GPUI**, for Linux and macOS.

## Features

- **Editing:** tabs, split views, line numbers, completion, and Vim or familiar controls.
- **Files and search:** project explorer, quick file opening, search and replace.
- **Git:** stage changes, commit, browse history, and manage branches and worktrees.
- **Terminals:** run commands without leaving the editor.
- **Build tools:** compile your project and jump to errors.
- **Previews:** read Markdown and PDFs beside your code.
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

## Help and feedback

- [Plugins](docs/plugins.md) · [Profiles](docs/profiles.md) · [Coding practice](docs/practice.md)
- Press **F12** to prepare a bug report, then review and submit it on [GitHub](https://github.com/Tuim-editor/Gtuim/issues).

## License

[GPL-3.0-or-later](LICENSE). Based on [Rtuim](https://github.com/Tuim-editor/Rtuim), with its [upstream MIT notice](LICENSES/Rtuim-MIT.txt) preserved.
