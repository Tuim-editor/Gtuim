# Plugin System & Self-Configuration

Gtuim uses `lazy.nvim` inside an isolated Neovim runtime (`NVIM_APPNAME=tuim`, data in `~/.local/share/tuim`, user config in `~/.config/tuim`) so your personal `~/.config/nvim` stays untouched.

## Plugin Center & Supported Recipes

Gtuim ships a curated recipe catalog ([src/nvim/plugin_recipes.json](../src/nvim/plugin_recipes.json), embedded into `~/.local/share/tuim/plugin_recipes.json`) with structured configuration forms, verified version pins, system requirement checks, and non-destructive runtime verification:

| Recipe | Repository | Version Pin | Notes |
| --- | --- | --- | --- |
| **Gruber Darker** | `blazkowolf/gruber-darker.nvim` | commit `35cb97959ef0` | Bold, underline, undercurl, italic, and invert toggles; appears in Settings > Theme |
| **Gitsigns** | `lewis6991/gitsigns.nvim` | tag `v2.1.0` | Gutter signs, blame, debounce & max-lines options; checks `git` availability |
| **Comment.nvim** | `numToStr/Comment.nvim` | tag `v0.8.0` | Padding, sticky cursor, and line/block toggle key options (`gcc` / `gbc`) |
| **Autopairs** | `windwp/nvim-autopairs` | commit `3172f40c1656` | Treesitter & bracket-line options; automatically yields Gtuim's basic built-in autopairs |

### Plugin Center UI, Lifecycle States & Persistence

Open **Plugin Center** (`Extensions / Plugins`) to browse **Installed**, **Supported recipes** (works completely offline), and **Community** repositories. Every plugin reports a structured `lifecycle` state (`not_installed`, `pending_restart`, `downloaded`, `dependencies_ready`, `initialized`, `verified`, `needs_attention`) and `health_message` in `plugin_inventory.json`:

- **Typed Recipe Forms (`~/.local/share/tuim/recipe_options.json`)**: Supported recipes open a typed form (booleans, range-validated integers with range-aware steps, enums, strings) with unsaved-change protection (`Ctrl+S` save, explicit discard confirmation). Flat dotted keys (e.g., `italic.strings`, `toggler.line`) are validated and saved atomically via `store_search.py configure <repo> <json>`, then expanded into nested Lua `opts` at startup **only when no explicit Lua `opts`/`config` override exists** in `~/.config/tuim/plugins.lua` or `~/.config/tuim/plugins/<owner>_<repo>.lua`. Use **Advanced Lua (L)** whenever you want full custom Lua specs.
- **Version Pinning (`~/.local/share/tuim/plugin_pins.json`)**: Pin precedence is `user` (`Pin installed revision` -> locks exact installed SHA from `lazy-lock.json` / `git rev-parse HEAD`) > `lua` (explicit `commit`/`tag`/`branch`/`version` in user Lua specs) > `recipe` (curated commit/tag in `plugin_recipes.json`). `Remove user pin` (`unpin`) reverts to Lua/recipe policy; restart Neovim to apply pin changes.
- **Non-Destructive Health Checks (`Refresh health (V)`)**: Checks CLI requirements, colorscheme files, and loaded module API functions (`type == "function"`) after `setup()` without mutating open buffers, switching the active theme, or triggering Lazy autoload on disabled/uninitialized plugins. A prior config/setup failure is never masked by symbol presence. Health messages report the exact checks performed rather than claiming interactive editing behavior.

## Default Install & Auto-Initialization

When you install a plugin from **Extensions** (or add a bare `"owner/repo"` spec in `~/.config/tuim/plugins.lua`):

- **Colorscheme plugins** (containing `colors/*.lua` or `colors/*.vim`):
  - Automatically detected as colorschemes (`auto_init = "colorscheme"`), loaded eagerly with `priority = 1000`, and automatically added to **Settings > Appearance > Theme**.
- **Lua plugins** (containing `lua/<module>.lua` or `lua/<module>/init.lua`):
  - Automatically detected and initialized via `pcall(require(mod).setup, opts or {})` unless you supply a custom `config`, `opts`, `main`, or set `tuim_auto_setup = false`.
  - If `require()` or default `setup()` fails, startup continues safely, the plugin is marked `needs_config` in `plugin_inventory.json`, and a warning notice directs you to **Configure (E)**.
- **Vimscript / runtime plugins** (no Lua `setup()`):
  - Loaded via Neovim's standard runtimepath (`auto_init = "runtime"`).
- **Direct GitHub install**:
  - In **Extensions > Discover**, typing any valid `owner/repo` slug offers a direct GitHub install entry even if the repository is not in the cached catalog.
- **Settings honored on boot**:
  - `Enable Lazy.nvim` and `Enable Mason (LSP/Tools)` in **Settings > Plugins** are read before plugin bootstrap and control whether Lazy plugins and Mason LSP auto-installs run.

## User Self-Configuration (`~/.config/tuim/`)

Respects `$XDG_CONFIG_HOME/tuim` (default `~/.config/tuim`):

| Path | Purpose | How to apply |
| --- | --- | --- |
| `~/.config/tuim/init.lua` | Personal Neovim options, keymaps, autocmds, colorscheme tweaks (runs after plugins & settings load) | Live reload with `:TuimReloadConfig` or `F1 > Config: Reload user init.lua` |
| `~/.config/tuim/plugins.lua` | Declarative `lazy.nvim` spec table or list of specs (adds new plugins or overrides bundled/marketplace specs) | `F1 > Plugins: Restart Neovim session` |
| `~/.config/tuim/plugins/<owner>_<repo>.lua` | Per-plugin override created by **Configure (E)** in Extensions or Lazy Manager; must return a `lazy.nvim` spec table or `function(plugin, opts)` | `F1 > Plugins: Restart Neovim session` |
| `~/.config/tuim/plugins/<custom>.lua` | Drop-in `lazy.nvim` spec files (LazyVim style) | `F1 > Plugins: Restart Neovim session` |

Existing legacy files in `~/.local/share/tuim/plugin_configs/<owner>_<repo>.lua` remain supported automatically.

### Notes & Caveats

- **Overriding bundled plugins**: Bundled specs that use a custom `config = function() ... end` closure (such as `alpha-nvim`, `blink.cmp`, `nvim-lspconfig`, and `telescope.nvim`) may ignore `opts`-only overrides; supply a `config` function in your override if you need to replace their setup completely.
- **System prerequisites & offline resilience**: Plugin bootstrap requires `git` on `PATH` and uses a bounded 15-second clone timeout with `GIT_TERMINAL_PROMPT=0` so offline or firewalled machines fall back cleanly without hanging startup. Treesitter parser builds and some Mason servers require host toolchains (e.g., C compiler, `git`, `node`/`python`); Gtuim never installs OS packages automatically.

### Quick Access

- **Settings > Plugins**: Toggle Lazy/Mason, open Mason / Lazy / Extensions, edit `plugins.lua` or `init.lua`, reload `init.lua`, or restart embedded Neovim.
- **Command Palette (`F1`)**: `Config: Edit user plugins.lua`, `Config: Edit user init.lua`, `Config: Reload user init.lua`, `Plugins: Open Lazy manager`, `Plugins: Show plugin health`, `Plugins: Restart Neovim session`.
- **Neovim Commands**: `:TuimEditInit`, `:TuimEditPlugins`, `:TuimReloadConfig`, `:TuimPluginHealth`.
- **Recovery / Safe Mode**: Launch with `TUIM_DISABLE_PLUGINS=1` to bypass all plugin specs and user plugin configs cleanly.

## Verification

Offline automated suites (no network, isolated temp XDG/HOME):

```sh
python3 scripts/test_plugin_runtime.py
python3 tests/test_plugin_store.py
cargo test --lib -- --test-threads=1
cargo build --release
```

Opt-in live upstream recipe verification (requires network access and executes the pinned third-party plugin code inside an isolated temporary `HOME`/`XDG` directory without touching your real `~/.config/tuim` or `~/.local/share/tuim`):

```sh
python3 scripts/test_plugin_recipes_live.py --online
```
