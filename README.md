# Gtuim

Native GPU-accelerated GUI rebuild of [Rtuim](https://github.com/Tuim-editor/Rtuim), built with **GPUI** (the Rust GUI framework powering [Zed](https://zed.dev/)) while preserving Neovim as the embedded editing engine and keeping full feature parity with the TUI app.

## Architecture

- **GUI Framework:** `gpui = "=0.2.2"` (Zed's GPU-accelerated Rust UI framework, native Wayland/X11/Vulkan rendering).
- **Embedded Editor Engine:** Single embedded Neovim process over MessagePack-RPC (`ext_linegrid` + `rgb: true`), reusing Tuim's isolated runtime (`NVIM_APPNAME=tuim`, `~/.local/share/tuim`).
- **GPU Grid & Terminal Canvas (`src/gui/grid_canvas.rs`):** Custom GPUI `Element` rendering Neovim's main + floating/split grids and both native `portable-pty` + `vt100` shell terminals with batched background quads, `ShapedLine` monospace shaping, truecolor highlights, mouse selection/scroll, and dynamic pixel-to-cell resizing.
- **Native GPUI Workspace Chrome (`src/gui/chrome.rs`, `src/gui/sidebar.rs`, `src/gui/modals.rs`):**
  - Multi-buffer tab bar with dirty indicators, close buttons, File/Edit/View/Tools/Help dropdown menus, split/terminal/command buttons, and clickable status bar mode pill (`IDE` / `NORMAL` / `INSERT` / `VISUAL` / `ZEN`).
  - Left Activity Rail + resizable Sidebar: Workspace Overview, File Explorer (tree, git badges, create/rename/delete, right-click menu), Git Source Control (stage/unstage/commit/pull/push/recent commits), Project Ripgrep Search, and AI Assistants (Agents/Context/Actions + session controls).
  - Resizable Bottom Drawer (`Terminal`, `Debug console`, `Output`) and Right Vertical Split Terminal with draggable pixel splitters and `Alt+Arrow` resizing.
  - Full modal suite: Command Palette (`F1`, `#` buffers, `:` line jump, `F2` live key rebinding), Settings (themes incl. Omarchy System theme, editor options, Vim-safe/Familiar/Factory keybinding presets), Plugin Center (supported recipes, typed configuration forms, lifecycle health, version pins, and community marketplace with trust confirmation), Mason Language Tools, Lazy Plugin Manager, Git Detailed (History & Branches), Worktree & Agent Orchestrator, Bug Report (`F12`), Editor Context Menu, and Toast Notices.

## Local profiles

Click the avatar in the top-right header to open the profile panel. Create and switch local profiles without closing editor buffers. Click the display name to rename it (Enter saves, Escape cancels); click the picture to select a local PNG, JPEG or WebP. Pictures are validated, resized and copied into profile storage, so the original can be moved or deleted. Built-in avatars remain available as a fallback.

Each profile has its own settings, local activity calendar, language statistics, and optional practice progress. Practice is **disabled until explicitly enabled for that profile**, including the initial default profile. The green calendar uses local calendar dates; click a day for its edit/save totals. Counts represent Neovim text-change batches and file saves—not Git commits, keystrokes, or lines of code.

See [profile storage and behavior](docs/profiles.md) for migration, privacy, concurrency and practice integration.

## Daily coding practice

Practice lives below the profile's statistics and is **opt-in per profile**, with separate persistent drafts and progress. Choose a language and difficulty, implement the supplied function, then use **Submit / Ctrl+Enter** to run local tests. Daily selection and streaks use your local calendar date.

See [practice documentation](docs/practice.md) for **C and Zig** toolchains, starter/harness conventions, other languages, and execution/trust limits. Local submissions run as your user with resource limits; they are not a security sandbox.

## Compile

Press **F1** and choose **Compile**, or open **Tools → Compile**. Gtuim detects the current file's language and suggests a build command using the project files it finds, such as `Cargo.toml` or `Makefile`. Edit the command and press Enter to run it. Custom commands are remembered for the current editor session.

The floating output window shows live output and the exit status. Press **q** or **Esc** to close it and stop the build if it is still running. Builds read files from disk, so save any changes you want included.

## Plugin Center

Open **Plugin Center** from the command palette, menu, or activity rail. **Supported recipes** currently cover Gruber Darker, Gitsigns, Comment.nvim, and nvim-autopairs, with pinned upstream versions, editable forms, and non-destructive runtime checks. Community plugins remain available with best-effort initialization and editable Lua.

Configuration forms save separately from explicit user Lua overrides. Installed-plugin changes require an explicitly confirmed Neovim restart; pre-configuring an uninstalled plugin does not. Version policy and installed revision are shown separately. Automatic rollback is not implemented yet.

See [Plugin Center documentation](docs/plugins.md) for configuration, lifecycle meanings, pinning, and offline/opt-in live tests.

## Markdown preview

Open a `.md`, `.markdown`, `.mdown`, or `.mkd` file and click **Markdown preview** above the editor, or press **Ctrl+Shift+M** while the editor is focused. The native preview appears beside the editable source; click **Hide Markdown preview** or use the shortcut again to close it. Buffers with Neovim's `markdown` filetype also work through the shortcut.

The preview follows the active buffer and includes **unsaved edits**, with updates coalesced over 120 ms. It renders headings, emphasis, inline/fenced code, lists, task lists, blockquotes, rules, and basic tables. Links display their destinations; images display alt text. Embedded HTML is shown as inert text, not executed, and remote resources are not fetched. Large previews are limited to 1 MiB / 20,000 lines, with a visible warning.

## PDF preview

Open a `.pdf` file from the explorer, file picker, or command line to view it directly inside the GUI. **Scroll normally with your mouse wheel or trackpad:** pages are stacked vertically in one continuous document, so you can move from one page to the next without turning pages. Only nearby pages are rendered, keeping large documents responsive. The page indicator follows the viewport, and page navigation jumps to the selected page in the same scrollable document.

To open **any PDF alongside your current source**, choose **View → Open PDF in Split…** or **F1 → Open PDF in Split…**, then select the file. Your source stays open and editable; the selected PDF is pinned in the adjacent pane. Continuous scrolling works in both full and split views. You can also use the PDF toolbar's **Split/Full** toggle, or open a matching sibling PDF (for example, `report.pdf` beside `report.tex`) using the preview toggle.

Use **Ctrl+Shift+D**, **F1 → Toggle PDF preview**, or **View → Toggle PDF Preview** to toggle the viewer. The toolbar provides previous/next page, zoom, fit-width/actual-size, reload, split/full view, and close controls. Visible PDFs automatically reload when the on-disk file changes.

In full PDF view, **Left/Right** or **PageUp/PageDown** jump to pages, **+/-** controls zoom, **0** resets zoom, **f** switches fit mode, and **r** reloads. In split view, editor typing remains with Neovim; scroll over the PDF pane or use its toolbar.

PDF rendering requires Poppler's `pdfinfo` and `pdftoppm` on `PATH` (Arch: `poppler`, Debian/Ubuntu: `poppler-utils`). Missing tools or unreadable/protected PDFs produce an in-app error rather than opening an external viewer.

## Build & Run

```bash
cd /home/roubs/Project/Gtuim
cargo run -- [FILES...]

# Or release build:
cargo build --release
./target/release/gtuim [FILES...]
```

## Bug reports

Press **F12** or choose **Help → Report Bug**. Reports open as a prefilled GitHub
issue draft; you review and submit them on GitHub using your own account. No
reporting server or API token is required. The default tracker is
`Tuim-editor/Rtuim`, the upstream project linked above. Set
`GTUIM_BUG_REPORT_REPO=owner/repository` before launching to choose another tracker.

Reports are intended for a **public issue tracker**. Opening a prefilled draft
sends its URL-embedded report content to GitHub and may save it in browser history,
even before you submit the issue. Home paths and common token patterns are
redacted on a best-effort basis, not guaranteed anonymized. Review
the report for private information before submitting. Copy Report provides the
full redacted report for manual submission, including when it is too long for a
browser URL. Opening a draft does **not** mean an issue has been submitted.
On the GUI confirmation/result screens, **C** copies the report; on results,
**R** returns to editing. After a draft request, **U** copies its URL and **O**
requests reopening it. The shared TUI widget additionally offers **E** to export
a complete redacted report to a private local Markdown file when clipboard or
browser helpers are unavailable. Exported reports remain local until you share them.

## Shareable builds

See [distribution instructions](docs/distribution.md) for runtime dependencies,
Linux/macOS native builds, packaging and the three-platform GitHub Actions
workflow. macOS binaries require native Apple SDK/Metal tooling; they cannot be
produced by simply renaming the Linux executable.

## Shortcuts

All Rtuim shortcuts carry over directly (`F1` Commands, `Ctrl+E` Explorer, `Ctrl+T` Bottom Terminal, `Ctrl+P` Find File, `Alt+G` Project Search, `F6`/`Shift+F6` Cycle Focus, `Alt+Arrows` Resize Panes, `F11` Zen Mode, `F12` Report Bug, `Ctrl+S` Save, `Ctrl+Q` Quit), plus `Ctrl+=` / `Ctrl+-` / `Ctrl+0` for GUI font zoom.
