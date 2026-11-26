-- Parser generators installed by setup.sh remain private to Tuim.
local tools_bin = vim.fn.stdpath("data") .. "/tools/bin"
vim.env.PATH = tools_bin .. ":" .. (vim.env.PATH or "")
local site = vim.fn.stdpath("data") .. "/site"
vim.opt.rtp:prepend(site)

local lazypath = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
local plugin_data = vim.fn.stdpath("data")
local function tuim_config_dir()
  local xdg = vim.env.XDG_CONFIG_HOME
  if xdg and xdg ~= "" then
    return xdg .. "/tuim"
  end
  local home = vim.env.HOME or vim.fn.expand("~")
  return home .. "/.config/tuim"
end
local user_config_dir = tuim_config_dir()
vim.g.tuim_config_dir = user_config_dir
_G.tuim_config_dir = tuim_config_dir

local function read_early_settings()
  local f = io.open(vim.g.tuim_settings_path or (plugin_data .. "/settings.json"), "r")
  if not f then return {} end
  local content = f:read("*a")
  f:close()
  local ok, parsed = pcall(vim.json.decode, content)
  if ok and type(parsed) == "table" then return parsed end
  return {}
end
local early_settings = read_early_settings()
vim.g.tuim_mason_enabled = early_settings.mason_enabled ~= false
local lazy_disabled_by_setting = early_settings.lazy_enabled == false
local plugins_disabled = os.getenv("TUIM_DISABLE_PLUGINS") == "1" or lazy_disabled_by_setting
local pending_plugin_notice = nil
local plugin_bootstrapped = false

_G.tuim_native_notice = function(level, message)
  pcall(vim.rpcnotify, 1, "tuim_notice", level, message)
end

local function clone_lazy_bounded(dest, timeout_ms)
  if vim.fn.executable("git") ~= 1 then
    return false, "git is not installed or not on PATH"
  end
  local cmd = {
    "git",
    "clone",
    "--filter=blob:none",
    "https://github.com/folke/lazy.nvim.git",
    "--branch=stable",
    dest,
  }
  if type(vim.system) == "function" then
    local ok, res = pcall(function()
      return vim.system(cmd, {
        timeout = timeout_ms or 15000,
        env = { GIT_TERMINAL_PROMPT = "0" },
      }):wait((timeout_ms or 15000) + 1000)
    end)
    if not ok or type(res) ~= "table" or res.code ~= 0 or not vim.uv.fs_stat(dest) then
      local detail = type(res) == "table" and (res.stderr or "") or tostring(res)
      detail = detail:gsub("%s+", " "):gsub("^%s+", ""):gsub("%s+$", "")
      return false, detail ~= "" and detail or "git clone timed out or failed"
    end
    return true, nil
  end
  vim.fn.system(cmd)
  if vim.v.shell_error ~= 0 or not vim.uv.fs_stat(dest) then
    return false, "git clone failed"
  end
  return true, nil
end

if not plugins_disabled and not vim.uv.fs_stat(lazypath) then
  local cloned, clone_err = clone_lazy_bounded(lazypath, 15000)
  if not cloned then
    plugins_disabled = true
    pending_plugin_notice = "Plugin bootstrap failed (" .. tostring(clone_err) .. "); starting offline. Retry from Settings > Plugins."
  else
    plugin_bootstrapped = true
  end
end
if vim.uv.fs_stat(lazypath) then vim.opt.rtp:prepend(lazypath) end

vim.g.mapleader = " "
vim.opt.hidden = true
vim.opt.shortmess:append("AI")
vim.opt.completeopt = { "menu", "menuone", "noselect" }

local set = vim.opt
set.relativenumber = true
set.number = true
set.tabstop = 4
set.shiftwidth = 4
set.autoindent = true
set.expandtab = true
set.ignorecase = true
set.smartcase = true
set.termguicolors = true
set.background = "dark"
set.signcolumn = "yes"
set.cursorline = false
vim.api.nvim_create_autocmd({ "VimEnter", "WinEnter", "BufWinEnter" }, {
  callback = function()
    vim.wo.cursorline = false
  end,
})
set.colorcolumn = ""
set.clipboard:append("unnamedplus")
if vim.fn.has("clipboard") == 0 then
  vim.defer_fn(function()
    _G.tuim_native_notice("warning", "No Neovim clipboard provider was detected; install xclip, xsel, wl-clipboard, or pbcopy support.")
  end, 150)
end
set.backspace = "indent,eol,start"
set.splitbelow = true
set.splitright = true
set.iskeyword:append("-")
set.scrolloff = 8
set.swapfile = false
set.backup = false
set.fixendofline = false
local undodir = vim.fn.stdpath("data") .. "/undo"
vim.fn.mkdir(undodir, "p")
set.undodir = undodir
set.undofile = true
set.incsearch = true
set.updatetime = 50

local user_plugins_path = vim.fn.stdpath("data") .. "/user_plugins.json"

-- One picker language for files, text, buffers and help. Neovim owns the
-- thin borders so they resize and stack with the actual floating windows.
local function configure_pickers()
    local actions = require("telescope.actions")
    local layout_actions = require("telescope.actions.layout")
    local function picker_colors()
        local function hl(name) return vim.api.nvim_get_hl(0, { name = name, link = false }) end
        local normal, float = hl("Normal"), hl("NormalFloat")
        local bg, fg = float.bg or normal.bg or 0x202328, normal.fg or 0xd7dce2
        local accent = hl("Function").fg or hl("Identifier").fg or fg
        local function blend(a, b, weight)
            local value = 0
            for _, shift in ipairs({ 16, 8, 0 }) do
                local av, bv = math.floor(a / 2 ^ shift) % 256, math.floor(b / 2 ^ shift) % 256
                value = value + math.floor(av * weight + bv * (1 - weight)) * 2 ^ shift
            end
            return value
        end
        local border = blend(fg, bg, 0.3)
        for _, name in ipairs({ "TelescopeNormal", "TelescopePromptNormal", "TelescopeResultsNormal", "TelescopePreviewNormal" }) do
            vim.api.nvim_set_hl(0, name, { fg = fg, bg = bg })
        end
        for _, name in ipairs({ "TelescopeBorder", "TelescopePromptBorder", "TelescopeResultsBorder", "TelescopePreviewBorder" }) do
            vim.api.nvim_set_hl(0, name, { fg = border, bg = bg })
        end
        for _, name in ipairs({ "TelescopeTitle", "TelescopePromptTitle", "TelescopeResultsTitle", "TelescopePreviewTitle" }) do
            vim.api.nvim_set_hl(0, name, { fg = fg, bg = bg, bold = true })
        end
        vim.api.nvim_set_hl(0, "TelescopeSelection", { fg = fg, bg = blend(accent, bg, 0.15), bold = true })
        vim.api.nvim_set_hl(0, "TelescopeSelectionCaret", { fg = accent, bg = blend(accent, bg, 0.15), bold = true })
        vim.api.nvim_set_hl(0, "TelescopeMatching", { fg = accent, bold = true })
        vim.api.nvim_set_hl(0, "TelescopePromptPrefix", { fg = accent, bg = bg })
    end
    require("telescope").setup({
        defaults = {
            sorting_strategy = "ascending",
            layout_strategy = "horizontal",
            layout_config = {
                width = function(_, columns) return math.min(140, math.max(1, columns - 4)) end,
                height = function(_, _, lines) return math.min(26, math.max(1, lines - 4)) end,
                horizontal = { prompt_position = "top", preview_width = 0.48, preview_cutoff = 110 },
            },
            border = true,
            borderchars = { "─", "│", "─", "│", "╭", "╮", "╯", "╰" },
            prompt_prefix = " > ",
            selection_caret = "> ",
            entry_prefix = "  ",
            path_display = { "filename_first" },
            results_title = false,
            dynamic_preview_title = true,
            winblend = 0,
            mappings = {
                i = { ["<Esc>"] = actions.close, ["<M-p>"] = layout_actions.toggle_preview },
                n = { ["<Esc>"] = actions.close, ["<M-p>"] = layout_actions.toggle_preview },
            },
        },
        pickers = {
            find_files = { prompt_title = "Find files", hidden = true },
            git_files = { prompt_title = "Project files" },
            oldfiles = { prompt_title = "Recent files", cwd_only = true },
            buffers = { prompt_title = "Open files", sort_mru = true, ignore_current_buffer = false },
            live_grep = { prompt_title = "Search project" },
            grep_string = { prompt_title = "Find text" },
            current_buffer_fuzzy_find = { prompt_title = "Search this file", previewer = false },
            help_tags = { prompt_title = "Help" },
            commands = { prompt_title = "Commands", previewer = false },
            diagnostics = { prompt_title = "Problems" },
        },
    })
    picker_colors()
    vim.api.nvim_create_autocmd("ColorScheme", {
        group = vim.api.nvim_create_augroup("TuimPickerColors", { clear = true }),
        callback = picker_colors,
    })
end
_G.tuim_configure_pickers = configure_pickers
_G.tuim_picker_action = function(action)
    for _, win in ipairs(vim.api.nvim_list_wins()) do
        local buf = vim.api.nvim_win_get_buf(win)
        if vim.bo[buf].filetype == "TelescopePrompt" then
            if action == "preview" then
                require("telescope.actions.layout").toggle_preview(buf)
            else
                local names = { open = "select_default", close = "close", mark = "toggle_selection" }
                local name = names[action]
                if name then require("telescope.actions")[name](buf) end
            end
            return
        end
    end
end

local user_plugins = {}
local up_f = io.open(user_plugins_path, "r")
if up_f then
    local content = up_f:read("*a")
    up_f:close()
    local ok, decoded = pcall(vim.json.decode, content)
    if ok and type(decoded) == "table" then
        user_plugins = decoded
    end
end

-- Keep this list available to the installer and the plugin build hook.
_G.tuim_default_parsers = {
    "bash", "c", "cpp", "css", "go", "html", "javascript", "json", "lua",
    "markdown", "markdown_inline", "python", "query", "rust", "tsx",
    "typescript", "vim", "vimdoc", "zig",
}
_G.tuim_install_parsers = function()
    local ts = require("nvim-treesitter")
    ts.setup({ install_dir = site })
    ts.install(_G.tuim_default_parsers):wait(300000)
    vim.opt.rtp:prepend(site) -- refresh lookup after creating the parser directory
    for _, lang in ipairs(_G.tuim_default_parsers) do
        assert(vim.treesitter.language.add(lang, { path = site .. "/parser/" .. lang .. ".so" }), "Treesitter parser unavailable: " .. lang)
        assert(vim.treesitter.query.get(lang, "highlights"), "Treesitter highlights unavailable: " .. lang)
    end
end

local plugins_setup = {
    {
        "goolord/alpha-nvim",
        lazy = false,
        priority = 1000,
        config = function()
            local dashboard = require("alpha.themes.dashboard")
            dashboard.section.header.val = function()
                return vim.fn.strcharpart('tuim / ' .. vim.fn.fnamemodify(vim.fn.getcwd(), ':t'), 0, math.max(8, vim.api.nvim_win_get_width(0) - 6))
            end
            dashboard.section.header.opts.hl = 'Normal'
            dashboard.section.buttons.opts.spacing = 0
            dashboard.config.layout = {
                { type = 'padding', val = 2 }, dashboard.section.header,
                { type = 'padding', val = 2 }, dashboard.section.buttons,
            }

            local function format_key(key)
                if not key or key == "" then return "None" end
                if key:sub(1, 1) == "<" and key:sub(-1) == ">" then
                    local content = key:sub(2, -2)
                    local parts = {}
                    while true do
                        if content:sub(1, 2) == "C-" then
                            table.insert(parts, "Ctrl")
                            content = content:sub(3)
                        elseif content:sub(1, 2) == "M-" then
                            table.insert(parts, "Alt")
                            content = content:sub(3)
                        elseif content:sub(1, 2) == "S-" then
                            table.insert(parts, "Shift")
                            content = content:sub(3)
                        else
                            break
                        end
                    end
                    if #content == 1 then
                        content = content:upper()
                    elseif content:lower() == "esc" then
                        content = "Esc"
                    elseif content:lower() == "cr" or content:lower() == "enter" then
                        content = "Enter"
                    elseif content:lower() == "space" then
                        content = "Space"
                    end
                    table.insert(parts, content)
                    return table.concat(parts, "+")
                end
                return key
            end

            _G.tuim_update_dashboard_keys = function()
                local path = vim.g.tuim_settings_path or (vim.fn.stdpath("data") .. "/settings.json")
                local f = io.open(path, "r")
                local state = {}
                if f then
                    local content = f:read("*a")
                    f:close()
                    local ok, s = pcall(vim.fn.json_decode, content)
                    if ok and type(s) == "table" then
                        state = s
                    end
                end
                
                local kb = state.keybindings or {}
                local raw_new = kb.new_file or "<C-n>"
                local raw_find = kb.find_file or "<C-p>"
                local raw_search = kb.search_project or "<M-g>"
                local raw_quit = kb.quit or "<C-q>"
                local raw_recent = "<C-r>"
                local raw_commands = kb.commands or "<F1>"
                if raw_commands == "" then raw_commands = "<F1>" end
                local raw_help = kb.help or "?"
                if raw_help == "" then raw_help = "?" end
                local width = math.max(12, math.min(44, vim.api.nvim_win_get_width(0) - 6))
                local height = vim.api.nvim_win_get_height(0)
                dashboard.config.layout[3].val = height < 18 and 1 or 2
                local recent_limit = math.max(0, math.min(5, height - 15))
                local function clip(text, limit)
                    while vim.fn.strdisplaywidth(text) > limit do
                        text = vim.fn.strcharpart(text, 0, vim.fn.strchars(text) - 1)
                    end
                    return text
                end
                local function button(key, label, command, muted)
                    local btn = dashboard.button(key, label, command)
                    local shortcut = format_key(key)
                    btn.val = clip(label, math.max(1, width - #shortcut - 2))
                    btn.opts.width = width
                    btn.opts.position = "center"
                    btn.opts.hl = muted and "TuimWelcomeMuted" or "Normal"
                    btn.opts.hl_shortcut = "TuimWelcomeMuted"
                    btn.opts.shortcut = shortcut
                    btn.opts.cursor = 0
                    return btn
                end
                local buttons = {
                    button(raw_new, "New file", "<cmd>enew<cr>"),
                    button(raw_find, "Find file", "<cmd>Telescope find_files<cr>"),
                    button(raw_search, "Search project", "<cmd>Telescope live_grep<cr>"),
                    button(raw_recent, "Recent files", "<cmd>Telescope oldfiles<cr>"),
                    { type = "padding", val = 1 },
                }
                local cwd, recent_count, seen = vim.fn.getcwd(), 0, {}
                for _, path in ipairs(vim.v.oldfiles) do
                    if recent_count >= recent_limit then break end
                    if not seen[path] and vim.fn.filereadable(path) == 1 and path:sub(1, #cwd + 1) == cwd .. '/' then
                        seen[path] = true
                        recent_count = recent_count + 1
                        local item = button(tostring(recent_count), vim.fn.fnamemodify(path, ':.'), nil)
                        item.on_press = function() vim.cmd.edit(vim.fn.fnameescape(path)) end
                        item.opts.keymap = { 'n', tostring(recent_count), item.on_press, { silent = true } }
                        table.insert(buttons, item)
                    end
                end
                if recent_count == 0 and recent_limit > 0 then
                    table.insert(buttons, { type = 'text', val = clip('No recent files in this project', width), opts = { position = 'center', hl = 'TuimWelcomeMuted' } })
                end
                table.insert(buttons, { type = "padding", val = 1 })
                table.insert(buttons, button(raw_commands, "Command menu", "<cmd>lua vim.rpcnotify(1, 'tuim_open_commands')<cr>", true))
                table.insert(buttons, button(raw_help, "Help", "<cmd>HelpMenu<cr>", true))
                table.insert(buttons, button(raw_quit, "Quit", "<cmd>lua _G.tuim_confirm_quit()<cr>", true))
                dashboard.section.buttons.val = buttons
                local content_height = 1 + dashboard.config.layout[3].val + #buttons
                dashboard.config.layout[1].val = math.max(0, math.floor((height - content_height) / 2))
            end

            _G.tuim_update_dashboard_keys()

            dashboard.opts.opts = {
                noautocmd = true,
            }
            require("alpha").setup(dashboard.config)
            local group = vim.api.nvim_create_augroup("TuimDashboard", { clear = true })
            local function welcome_colors()
                local normal = vim.api.nvim_get_hl(0, { name = 'Normal', link = false })
                local fg, bg = normal.fg or 0xd4d4d4, normal.bg or 0x1e1e1e
                local muted = 0
                for _, shift in ipairs({16, 8, 0}) do
                    local f = bit.band(bit.rshift(fg, shift), 255)
                    local b = bit.band(bit.rshift(bg, shift), 255)
                    muted = muted + bit.lshift(math.floor(b + (f - b) * 0.7), shift)
                end
                vim.api.nvim_set_hl(0, 'TuimWelcomeMuted', { fg = muted })
            end
            welcome_colors()
            vim.api.nvim_create_autocmd('ColorScheme', { group = group, callback = welcome_colors })
            vim.api.nvim_create_autocmd({ 'VimResized', 'WinResized', 'DirChanged' }, {
                group = group,
                callback = function()
                    if vim.bo.filetype == 'alpha' then
                        _G.tuim_update_dashboard_keys()
                        pcall(function() require('alpha').redraw() end)
                    end
                end,
            })
            vim.api.nvim_create_autocmd({ "FileType" }, {
                group = group,
                pattern = "alpha",
                callback = function()
                    vim.cmd("setlocal nonumber norelativenumber") ;
                    vim.keymap.set("n", "<LeftRelease>", "<LeftRelease><cmd>lua pcall(function() require('alpha').press() end)<CR>", { buffer = true, silent = true })
                    if _G.tuim_update_dashboard_keys then
                        _G.tuim_update_dashboard_keys()
                        pcall(function() require("alpha").redraw() end)
                    end
                end,
            })
        end
    },
    { "nvim-lua/plenary.nvim", lazy = true },
    {
        "nvim-telescope/telescope.nvim",
        cmd = "Telescope",
        dependencies = { "nvim-lua/plenary.nvim" },
        config = configure_pickers,
        keys = {
            { "<leader>ff", "<cmd>Telescope find_files<cr>" },
            { "<leader>fg", "<cmd>Telescope live_grep<cr>" },
            { "<leader>fb", "<cmd>Telescope buffers<cr>" },
            { "<leader>fh", "<cmd>Telescope help_tags<cr>" },
        }
    },
    {
        "nvim-treesitter/nvim-treesitter",
        branch = "main",
        commit = "8b98b4470eb326f1c7b50dae79f8c963568e5720",
        lazy = false,
        build = function() _G.tuim_install_parsers() end,
        config = function()
            require("nvim-treesitter").setup({ install_dir = site })
            local function highlight(buf)
                if vim.bo[buf].buftype ~= "" or vim.bo[buf].filetype == "" then return end
                -- Uninstalled languages keep Vim's syntax fallback. Core parsers
                -- are installed and verified synchronously by setup.sh.
                pcall(vim.treesitter.start, buf)
            end
            vim.api.nvim_create_autocmd({ 'FileType', 'BufWinEnter' }, {
                group = vim.api.nvim_create_augroup('TuimTreesitter', { clear = true }),
                callback = function(event) highlight(event.buf) end,
            })
            highlight(vim.api.nvim_get_current_buf())
        end,
    },
    {
        "williamboman/mason.nvim",
        cmd = "Mason",
        cond = function() return vim.g.tuim_mason_enabled ~= false end,
        keys = { { "<leader>cm", "<cmd>Mason<cr>", desc = "Mason" } },
        config = true,
    },
    {
        "saghen/blink.cmp",
        lazy = false,
        cond = not vim.g.tuim_is_terminal,
        dependencies = "rafamadriz/friendly-snippets",
        version = "*",
        config = function()
            require("blink.cmp").setup({
                keymap = {
                    preset = "none",
                    ["<C-space>"] = { "show", "show_documentation", "hide_documentation" },
                    ["<C-e>"] = { "hide" },
                    ["<CR>"] = {
                        "accept",
                        function()
                            if vim.g.tuim_ide_mode and vim.bo.buftype == "" then
                                vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes("<CR><C-g>u", true, false, true), "ni", false)
                                return true
                            end
                        end,
                        "fallback",
                    },
                    ["<Tab>"] = { "select_next", "snippet_forward", "fallback" },
                    ["<S-Tab>"] = { "select_prev", "snippet_backward", "fallback" },
                    ["<C-b>"] = { "scroll_documentation_up", "fallback" },
                    ["<C-f>"] = { "scroll_documentation_down", "fallback" },
                },
                appearance = {
                    use_nvim_cmp_as_default = true,
                    nerd_font_variant = "mono",
                },
                sources = {
                    default = { "lsp", "path", "snippets", "buffer" },
                },
                enabled = function()
                    return vim.g.tuim_autocomplete_enabled ~= false
                end,
            })
        end,
    },
    {
        "neovim/nvim-lspconfig",
        lazy = false,
        cond = not vim.g.tuim_is_terminal,
        dependencies = {
            "williamboman/mason.nvim",
            "williamboman/mason-lspconfig.nvim",
            "saghen/blink.cmp",
        },
        config = function()
            if vim.g.tuim_mason_enabled == false then
                return
            end

            local lsp_ok, mason_lspconfig = pcall(require, "mason-lspconfig")
            if not lsp_ok then
                _G.tuim_native_notice("error", "LSP setup is unavailable because mason-lspconfig failed to load.")
                return
            end

            local mason_bin = vim.fn.stdpath("data") .. "/mason/bin"
            if vim.fn.isdirectory(mason_bin) == 1 then
                vim.env.PATH = mason_bin .. ":" .. vim.env.PATH
            end

            local mason_ok, mason_err = pcall(function()
                require("mason").setup({
                    install_root_dir = vim.fn.stdpath("data") .. "/mason",
                })
            end)
            if not mason_ok then
                _G.tuim_native_notice("error", "Mason setup failed; open Settings > Plugins and retry after checking the log.")
                vim.schedule(function() vim.notify(tostring(mason_err), vim.log.levels.ERROR) end)
                return
            end

            local capabilities = require("blink.cmp").get_lsp_capabilities()

            -- Apply completion capabilities + broad root_markers to all servers.
            -- root_markers is how Neovim 0.12 native LSP determines the project root.
            vim.lsp.config('*', {
                capabilities = capabilities,
                root_markers = {
                    '.git',
                    'pyproject.toml', 'setup.py', 'setup.cfg', 'requirements.txt', 'Pipfile',
                    'Cargo.toml', 'Cargo.lock',
                    'package.json', 'yarn.lock', 'package-lock.json',
                    'compile_commands.json', 'compile_flags.txt',
                    '.clangd', '.clang-tidy',
                    'build.zig', 'build.zig.zon',
                    '.luarc.json', '.luarc.jsonc',
                    'pyrightconfig.json',
                    'Makefile', 'CMakeLists.txt',
                    'go.mod', 'go.sum',
                    '.hg', '.svn',
                },
            })

            -- Configure lua_ls settings natively
            vim.lsp.config("lua_ls", {
                settings = {
                    Lua = {
                        diagnostics = { globals = { "vim" } },
                    },
                },
            })

            local project_servers = {}
            local seen_servers = {}
            local function recommend(server)
                if not seen_servers[server] then
                    seen_servers[server] = true
                    table.insert(project_servers, server)
                end
            end
            local cwd = vim.fn.getcwd()
            local marker_servers = {
                ["build.zig"] = "zls",
                [".luarc.json"] = "lua_ls",
                ["stylua.toml"] = "lua_ls",
                ["pyproject.toml"] = "pyright",
                ["requirements.txt"] = "pyright",
                ["Cargo.toml"] = "rust_analyzer",
                ["package.json"] = "ts_ls",
                ["go.mod"] = "gopls",
                ["compile_commands.json"] = "clangd",
                ["CMakeLists.txt"] = "clangd",
            }
            for marker, server in pairs(marker_servers) do
                if vim.uv.fs_stat(cwd .. "/" .. marker) then recommend(server) end
            end
            if #project_servers == 0 then
                local ext_server = {
                    zig = "zls", lua = "lua_ls", py = "pyright", rs = "rust_analyzer",
                    js = "ts_ls", jsx = "ts_ls", ts = "ts_ls", tsx = "ts_ls",
                    go = "gopls", c = "clangd", h = "clangd", cpp = "clangd", hpp = "clangd",
                }
                local ext = vim.fn.expand("%:e"):lower()
                if ext_server[ext] then recommend(ext_server[ext]) end
            end
            vim.g.tuim_recommended_servers = project_servers

            mason_lspconfig.setup({
                ensure_installed = project_servers,
                automatic_enable = true,
            })

            if #project_servers > 0 then
                vim.defer_fn(function()
                    _G.tuim_native_notice("info", "Project language tools: " .. table.concat(project_servers, ", "))
                end, 200)
            else
                vim.defer_fn(function()
                    _G.tuim_native_notice("info", "No project language markers detected; choose tools manually in Mason.")
                end, 200)
            end

            -- Enable all installed servers immediately (mason-lspconfig's async refresh
            -- sometimes misses servers on first load)
            local function enable_all()
                local servers = mason_lspconfig.get_installed_servers()
                if #servers > 0 then
                    vim.lsp.enable(servers)
                    pcall(function()
                        for _, bufnr in ipairs(vim.api.nvim_list_bufs()) do
                            if vim.api.nvim_buf_is_valid(bufnr) and vim.bo[bufnr].buflisted then
                                local ft = vim.bo[bufnr].filetype
                                if ft and ft ~= "" then
                                    vim.api.nvim_exec_autocmds("FileType", { buffer = bufnr })
                                end
                            end
                        end
                    end)
                end
            end

            enable_all()
            vim.defer_fn(enable_all, 500)

            -- Also re-enable on BufReadPost so newly installed servers attach
            vim.api.nvim_create_autocmd("BufReadPost", {
                once = false,
                callback = enable_all,
            })
        end,

    },
    {
        "ThePrimeagen/harpoon",
        keys = {
            { "<leader>a", function() require("harpoon.mark").add_file() end },
            { "<C-e>", function() require("harpoon.ui").toggle_quick_menu() end },
            { "<C-h>", function() require("harpoon.ui").nav_file(1) end },
            { "<C-j>", function() require("harpoon.ui").nav_file(2) end },
            { "<C-k>", function() require("harpoon.ui").nav_file(3) end },
            { "<C-l>", function() require("harpoon.ui").nav_file(4) end },
        }
    },
    { "Mofiqul/vscode.nvim", lazy = false, priority = 1000 },
    { "folke/tokyonight.nvim", lazy = true },
    { "catppuccin/nvim", name = "catppuccin", lazy = true },
    { "ellisonleao/gruvbox.nvim", lazy = true },
    { "shaunsingh/nord.nvim", lazy = true },
    { "scottmckendry/cyberdream.nvim", lazy = true },
    { "rose-pine/neovim", name = "rose-pine", lazy = true },
    { "rebelot/kanagawa.nvim", lazy = true },
    { "EdenEast/nightfox.nvim", lazy = true },
    { "tahayvr/matteblack.nvim", lazy = true },
}
-- Persist desired plugin state separately from plugin code and user options.
local lazy_root = plugin_data .. "/lazy"
local function read_plugin_json(name, fallback)
    local file = io.open(plugin_data .. "/" .. name, "r")
    if not file then return fallback end
    local content = file:read("*a")
    file:close()
    local ok, result = pcall(vim.json.decode, content)
    if ok and type(result) == "table" then return result end
    _G.tuim_native_notice("error", "Cannot read " .. name .. "; repair it before changing plugins.")
    return fallback
end

local function list_dir_entries(path)
    local out = {}
    local handle = vim.uv.fs_scandir(path)
    if not handle then return out end
    while true do
        local name, kind = vim.uv.fs_scandir_next(handle)
        if not name then break end
        table.insert(out, { name = name, kind = kind })
    end
    table.sort(out, function(a, b) return a.name < b.name end)
    return out
end

local function detect_plugin_colorschemes(dir)
    local schemes = {}
    local seen = {}
    for _, item in ipairs(list_dir_entries(dir .. "/colors")) do
        local stem = item.name:match("^(.+)%.lua$") or item.name:match("^(.+)%.vim$")
        if stem and not seen[stem] then
            seen[stem] = true
            table.insert(schemes, stem)
        end
    end
    return schemes
end

local function detect_plugin_lua_module(dir, repo)
    local lua_dir = dir .. "/lua"
    local entries = list_dir_entries(lua_dir)
    if #entries == 0 then return nil end

    local available = {}
    local lower_map = {}
    for _, item in ipairs(entries) do
        local mod = nil
        if item.kind == "directory" then
            if vim.uv.fs_stat(lua_dir .. "/" .. item.name .. "/init.lua") then
                mod = item.name
            end
        elseif item.name:sub(-4) == ".lua" then
            mod = item.name:sub(1, -5)
        end
        if mod and mod ~= "health" then
            available[mod] = true
            lower_map[mod:lower()] = mod
        end
    end

    local repo_name = (repo:match("([^/]+)$") or repo):gsub("%.git$", "")
    local stripped = repo_name:gsub("%.nvim$", ""):gsub("%.lua$", ""):gsub("%-nvim$", ""):gsub("^nvim%-", ""):gsub("^vim%-", "")
    local candidates = {
        repo_name,
        (repo_name:gsub("%.nvim$", ""):gsub("%.lua$", "")),
        stripped,
        (stripped:gsub("%-", "_")),
        (stripped:gsub("_", "-")),
    }
    for _, cand in ipairs(candidates) do
        if type(cand) == "string" then
            if available[cand] then return cand end
            if lower_map[cand:lower()] then return lower_map[cand:lower()] end
        end
    end

    local names = {}
    for mod in pairs(available) do table.insert(names, mod) end
    table.sort(names)
    if #names == 1 then return names[1] end
    return nil
end

local function resolve_plugin_config_path(repo)
    local sanitized = repo:gsub("/", "_")
    local primary = user_config_dir .. "/plugins/" .. sanitized .. ".lua"
    if vim.fn.filereadable(primary) == 1 then return primary end
    local legacy = plugin_data .. "/plugin_configs/" .. sanitized .. ".lua"
    if vim.fn.filereadable(legacy) == 1 then return legacy end
    return nil
end

local plugin_states = read_plugin_json("plugin_states.json", {})
local recipe_options = read_plugin_json("recipe_options.json", {})
local plugin_pins = read_plugin_json("plugin_pins.json", {})
local lazy_lock = read_plugin_json("lazy-lock.json", {})
local plugin_inventory = {}
local specs_by_repo = {}
local consumed_user_files = {}
local user_overrode_opts = {}
local user_overrode_config = {}
local user_overrode_pin = {}

local function load_plugin_recipes()
    local candidates = { plugin_data .. "/plugin_recipes.json" }
    local src_info = debug.getinfo(1, "S")
    if src_info and type(src_info.source) == "string" and src_info.source:sub(1, 1) == "@" then
        local dir = vim.fn.fnamemodify(src_info.source:sub(2), ":h")
        table.insert(candidates, dir .. "/plugin_recipes.json")
    end
    for _, path in ipairs(candidates) do
        local f = io.open(path, "r")
        if f then
            local content = f:read("*a")
            f:close()
            local ok, parsed = pcall(vim.json.decode, content)
            if ok and type(parsed) == "table" and type(parsed.recipes) == "table" then
                local by_repo = {}
                for _, recipe in ipairs(parsed.recipes) do
                    if type(recipe) == "table" and type(recipe.repo) == "string" then
                        by_repo[recipe.repo] = recipe
                    end
                end
                return by_repo
            end
        end
    end
    return {}
end

local recipes_by_repo = load_plugin_recipes()

local function set_nested_option(root, dotted_key, value)
    local parts = {}
    for segment in dotted_key:gmatch("[^%.]+") do
        table.insert(parts, segment)
    end
    if #parts == 0 then return end
    local cur = root
    for i = 1, #parts - 1 do
        local k = parts[i]
        if type(cur[k]) ~= "table" then
            cur[k] = {}
        end
        cur = cur[k]
    end
    cur[parts[#parts]] = value
end

local function expand_recipe_options(recipe, saved_values)
    local out = {}
    local saved = type(saved_values) == "table" and saved_values or {}
    for _, field in ipairs(recipe.fields or {}) do
        if type(field) == "table" and type(field.key) == "string" then
            local val = saved[field.key]
            if val == nil then
                val = field.default
            end
            if val ~= nil then
                set_nested_option(out, field.key, val)
            end
        end
    end
    return out
end

local function check_recipe_requirements(recipe)
    local missing = {}
    if type(recipe) ~= "table" then return missing end
    for _, req in ipairs(recipe.requirements or {}) do
        if type(req) == "table" and type(req.command) == "string" and req.command ~= "" then
            if vim.fn.executable(req.command) ~= 1 then
                table.insert(missing, req.label or req.command)
            end
        end
    end
    return missing
end

local function valid_commit_sha(sha)
    return type(sha) == "string" and sha:match("^[0-9a-fA-F]+$") ~= nil and #sha >= 7 and #sha <= 40
end

local function verify_recipe_plugin(recipe, plugin, entry)
    if type(recipe) ~= "table" or type(entry) ~= "table" then return false end
    local missing = check_recipe_requirements(recipe)
    entry.missing_requirements = missing
    if #missing > 0 then
        entry.verified = false
        entry.verify_failed = true
        entry.lifecycle = "needs_attention"
        entry.health_message = "Missing requirement(s): " .. table.concat(missing, ", ")
        return false
    end

    local ver = type(recipe.verification) == "string" and recipe.verification or ""
    local pdir = (plugin and plugin.dir) or (lazy_root .. "/" .. entry.name)

    if ver:sub(1, 12) == "colorscheme:" then
        local scheme = ver:sub(13)
        local has_file = vim.uv.fs_stat(pdir .. "/colors/" .. scheme .. ".lua") ~= nil
            or vim.uv.fs_stat(pdir .. "/colors/" .. scheme .. ".vim") ~= nil
        local mod_ok = true
        if type(recipe.module) == "string" and recipe.module ~= "" then
            mod_ok = package.loaded[recipe.module] ~= nil or pcall(require, recipe.module)
        end
        if has_file and mod_ok then
            entry.verified = true
            entry.verify_failed = false
            entry.lifecycle = "verified"
            entry.health_message = "Non-destructive check passed: colors/" .. scheme .. " file present"
                .. ((type(recipe.module) == "string" and recipe.module ~= "") and (" and module '" .. recipe.module .. "' loaded") or "")
                .. " (active theme unchanged)"
            return true
        else
            entry.verified = false
            entry.verify_failed = true
            entry.lifecycle = "needs_attention"
            entry.health_message = "Non-destructive check failed: colorscheme '" .. scheme .. "' file or module unavailable"
            return false
        end
    elseif ver:sub(1, 7) == "module:" then
        local rest = ver:sub(8)
        local mod_name, fns = rest:match("^([^:]+):?(.*)$")
        if mod_name and mod_name ~= "" then
            local ok, mod = true, package.loaded[mod_name]
            if mod == nil then
                ok, mod = pcall(require, mod_name)
            end
            if not ok or type(mod) ~= "table" then
                entry.verified = false
                entry.verify_failed = true
                entry.lifecycle = "needs_attention"
                entry.health_message = "Non-destructive API check failed loading '" .. mod_name .. "': " .. tostring(mod):gsub("\n.*", "")
                return false
            end
            if fns and fns ~= "" then
                for fn_name in fns:gmatch("[^,]+") do
                    if type(mod[fn_name]) ~= "function" then
                        entry.verified = false
                        entry.verify_failed = true
                        entry.lifecycle = "needs_attention"
                        entry.health_message = "Non-destructive API check failed: " .. mod_name .. "." .. fn_name .. " is not a function"
                        return false
                    end
                end
            end
            entry.verified = true
            entry.verify_failed = false
            entry.lifecycle = "verified"
            local exports_note = (fns and fns ~= "") and (" (API functions: " .. fns .. ")") or ""
            entry.health_message = "Non-destructive API check passed: module '" .. mod_name .. "' loaded" .. exports_note .. " after setup()"
            return true
        end
    end

    entry.verified = false
    entry.verify_failed = false
    entry.lifecycle = "initialized"
    entry.health_message = "Initialized"
    return true
end

local function register_spec(spec, source, parent)
    if type(spec) == "string" then spec = { spec } end
    if type(spec) ~= "table" or type(spec[1]) ~= "string" then return spec end
    local repo = spec[1]
    local name = spec.name or repo:match("([^/]+)$"):gsub("%.git$", "")
    local recipe = recipes_by_repo[repo]
    local entry = plugin_inventory[repo]
    if not entry then
        local lock_entry = type(lazy_lock[name]) == "table" and lazy_lock[name] or nil
        local saved_opts = type(recipe_options[repo]) == "table" and recipe_options[repo] or nil
        entry = {
            name = name,
            full_name = repo,
            source = source,
            required_by = {},
            description = spec.desc or (recipe and recipe.description) or "",
            enabled = plugin_states[repo] ~= "disabled",
            removed = plugin_states[repo] == "removed",
            auto_init = source == "Bundled" and "bundled" or "pending",
            config_path = "",
            colorschemes = {},
            lua_module = (recipe and recipe.module) or "",
            supported = recipe ~= nil,
            recipe_id = (recipe and recipe.id) or "",
            lifecycle = "downloaded",
            health_message = "",
            installed_commit = (lock_entry and type(lock_entry.commit) == "string") and lock_entry.commit or "",
            pinned_commit = "",
            pin_source = "",
            configured = saved_opts ~= nil and next(saved_opts) ~= nil,
            verified = false,
            config_error = false,
            missing_requirements = recipe and check_recipe_requirements(recipe) or {},
        }
        plugin_inventory[repo] = entry
    end
    if not specs_by_repo[repo] then
        specs_by_repo[repo] = spec
    end
    if parent and not vim.tbl_contains(entry.required_by, parent) then
        table.insert(entry.required_by, parent)
    end
    local deps = spec.dependencies
    if type(deps) == "string" then deps = { deps } end
    if type(deps) == "table" then
        for i, dep in ipairs(deps) do deps[i] = register_spec(dep, "Dependency", repo) end
        spec.dependencies = deps
    end
    if recipe and type(recipe.dependencies) == "table" and #recipe.dependencies > 0 then
        spec.dependencies = type(spec.dependencies) == "table" and spec.dependencies or {}
        for _, dep_repo in ipairs(recipe.dependencies) do
            if type(dep_repo) == "string" then
                local present = false
                for _, existing in ipairs(spec.dependencies) do
                    if type(existing) == "table" and existing[1] == dep_repo then
                        present = true
                        break
                    end
                end
                if not present then
                    table.insert(spec.dependencies, register_spec({ dep_repo }, "Dependency", repo))
                end
            end
        end
    end
    return spec
end

local function merge_custom_spec(target, custom, source_label, parent_repo)
    if type(custom) == "function" then
        target.config = custom
        if parent_repo then user_overrode_config[parent_repo] = true end
        return
    end
    if type(custom) ~= "table" then return end
    local protected_keys = { name = true, dir = true, url = true, enabled = true, cond = true }
    if source_label == "User config" then
        protected_keys.name = nil
    end
    for key, value in pairs(custom) do
        if type(key) == "string" and not protected_keys[key] then
            if key == "opts" then
                if parent_repo then user_overrode_opts[parent_repo] = true end
                if type(value) == "table" and type(target.opts) == "table" then
                    target.opts = vim.tbl_deep_extend("force", target.opts, value)
                else
                    target.opts = value
                end
            elseif key == "config" then
                if parent_repo then user_overrode_config[parent_repo] = true end
                target[key] = value
            elseif key == "commit" or key == "tag" or key == "branch" or key == "version" then
                if parent_repo then user_overrode_pin[parent_repo] = true end
                target[key] = value
            elseif key == "dependencies" then
                local incoming = type(value) == "string" and { value } or (type(value) == "table" and value or {})
                target.dependencies = type(target.dependencies) == "table" and target.dependencies or {}
                for _, dep in ipairs(incoming) do
                    local reg = register_spec(dep, "Dependency", parent_repo)
                    table.insert(target.dependencies, reg)
                end
            else
                target[key] = value
            end
        end
    end
end

local function is_single_plugin_spec(tbl)
    if type(tbl) ~= "table" then return false end
    if type(tbl[1]) == "string" then return true end
    for _, key in ipairs({ "opts", "config", "init", "keys", "cmd", "ft", "event", "dependencies", "lazy", "priority", "branch", "tag", "commit", "version", "build", "main", "tuim_auto_setup" }) do
        if tbl[key] ~= nil then return true end
    end
    return false
end

local function ingest_user_spec_list(value, file_path)
    local items = {}
    if type(value) == "string" then
        items = { { value } }
    elseif type(value) == "table" then
        if is_single_plugin_spec(value) then
            items = { value }
        else
            for _, item in ipairs(value) do
                if type(item) == "string" then
                    table.insert(items, { item })
                elseif type(item) == "table" and type(item[1]) == "string" then
                    table.insert(items, item)
                end
            end
        end
    end
    for _, raw in ipairs(items) do
        local repo = raw[1]
        if type(repo) == "string" and repo:match("^[%w_.-]+/[%w_.-]+$") then
            if specs_by_repo[repo] then
                merge_custom_spec(specs_by_repo[repo], raw, "User config", repo)
                local entry = plugin_inventory[repo]
                if entry then
                    entry.config_path = file_path
                    entry.configured = true
                    entry.auto_init = "custom"
                end
            else
                if raw.opts ~= nil then user_overrode_opts[repo] = true end
                if raw.config ~= nil then user_overrode_config[repo] = true end
                if raw.commit ~= nil or raw.tag ~= nil or raw.branch ~= nil or raw.version ~= nil then
                    user_overrode_pin[repo] = true
                end
                local reg = register_spec(raw, "User config")
                table.insert(plugins_setup, reg)
                local entry = plugin_inventory[repo]
                if entry then
                    entry.config_path = file_path
                    entry.configured = true
                    if reg.config ~= nil or reg.opts ~= nil then
                        entry.auto_init = "custom"
                    end
                end
            end
        end
    end
end

for i, spec in ipairs(plugins_setup) do plugins_setup[i] = register_spec(spec, "Bundled") end
for _, repo in ipairs(user_plugins) do
    if type(repo) == "string" and not plugin_inventory[repo] then
        table.insert(plugins_setup, register_spec({ repo }, "Marketplace"))
    end
end
-- Dependency specs discovered by Lazy can also receive persistent overrides.
for repo, _ in pairs(plugin_states) do
    if type(repo) == "string" and repo:match("^[%w_.-]+/[%w_.-]+$") and repo ~= "folke/lazy.nvim" and not plugin_inventory[repo] then
        table.insert(plugins_setup, register_spec({ repo }, "Dependency"))
    end
end
register_spec({ "folke/lazy.nvim" }, "Plugin manager")

-- Load user's declarative ~/.config/tuim/plugins.lua (standard lazy.nvim spec list).
local user_plugins_lua = user_config_dir .. "/plugins.lua"
if not plugins_disabled and vim.fn.filereadable(user_plugins_lua) == 1 then
    local chunk, load_err = loadfile(user_plugins_lua)
    if not chunk then
        _G.tuim_native_notice("error", "Syntax error in " .. user_plugins_lua .. ": " .. tostring(load_err))
    else
        local ok, result = pcall(chunk)
        if not ok then
            _G.tuim_native_notice("error", "Error loading " .. user_plugins_lua .. ": " .. tostring(result))
        elseif result ~= nil then
            ingest_user_spec_list(result, user_plugins_lua)
        end
    end
end

-- Per-plugin overrides (both ~/.config/tuim/plugins/<owner>_<repo>.lua and legacy data/plugin_configs/).
-- Also supports curated recipes (pinned versions, structured recipe_options.json, non-destructive verification)
-- and smart default auto-init for Marketplace / bare user plugins.
local active_theme_name = early_settings.theme or "vscode"
local function apply_pin_policy(spec, entry, recipe)
    local repo = spec[1]
    local user_pin = plugin_pins[repo]
    if valid_commit_sha(user_pin) then
        spec.commit = user_pin
        spec.tag = nil
        spec.branch = nil
        spec.version = nil
        if entry then
            entry.pinned_commit = user_pin
            entry.pin_source = "user"
        end
        return
    end
    if user_overrode_pin[repo] then
        local lua_ref = spec.commit or spec.tag or spec.branch or spec.version
        if entry then
            entry.pinned_commit = lua_ref ~= nil and tostring(lua_ref) or "Lua-managed"
            entry.pin_source = "lua"
        end
        return
    end
    if recipe and type(recipe.pin) == "string" and recipe.pin ~= "" then
        if recipe.pin_kind == "commit" and valid_commit_sha(recipe.pin) then
            spec.commit = recipe.pin
            if entry then
                entry.pinned_commit = recipe.pin
                entry.pin_source = "recipe"
            end
        elseif recipe.pin_kind == "tag" then
            spec.tag = recipe.pin
            if entry then
                entry.pinned_commit = recipe.pin
                entry.pin_source = "recipe"
            end
        end
    end
end

local function apply_smart_default_init(spec, entry)
    local repo = spec[1]
    local recipe = recipes_by_repo[repo]
    local dir = lazy_root .. "/" .. (entry and entry.name or repo:match("([^/]+)$"))
    local on_disk = vim.uv.fs_stat(dir) ~= nil

    apply_pin_policy(spec, entry, recipe)

    if on_disk and entry then
        local schemes = detect_plugin_colorschemes(dir)
        entry.colorschemes = schemes
        local mod = detect_plugin_lua_module(dir, repo)
        entry.lua_module = mod or entry.lua_module or ""
        if #schemes > 0 then
            if vim.tbl_contains(schemes, active_theme_name) or entry.source ~= "Bundled" then
                if spec.lazy == nil then spec.lazy = false end
                if spec.priority == nil then spec.priority = 1000 end
            end
        end
    end

    if entry and entry.source == "Bundled" then
        return
    end

    if recipe then
        local saved_opts = type(recipe_options[repo]) == "table" and recipe_options[repo] or nil
        if saved_opts and next(saved_opts) ~= nil and entry then
            entry.configured = true
        end
        -- Merge recipe defaults + saved recipe_options.json only when user did not supply explicit opts/config override.
        if not user_overrode_opts[repo] and not user_overrode_config[repo] then
            spec.opts = expand_recipe_options(recipe, saved_opts)
        end
        if recipe.category == "colorscheme" then
            if spec.lazy == nil then spec.lazy = false end
            if spec.priority == nil then spec.priority = 1000 end
        elseif spec.lazy == nil and spec.cmd == nil and spec.ft == nil and spec.event == nil and spec.keys == nil then
            spec.lazy = false
        end

        if spec.tuim_auto_setup == false then
            if entry then entry.auto_init = "disabled" end
            return
        end

        if user_overrode_config[repo] and type(spec.config) == "function" then
            local user_cfg = spec.config
            spec.config = function(plugin, opts)
                local ok, err = pcall(user_cfg, plugin, opts)
                if not ok then
                    if entry then
                        entry.auto_init = "needs_config"
                        entry.config_error = true
                        entry.lifecycle = "needs_attention"
                        entry.health_message = "Custom config failed: " .. tostring(err):gsub("\n.*", "")
                    end
                    vim.schedule(function()
                        _G.tuim_native_notice("error", "Custom config failed for " .. repo .. ": " .. tostring(err):gsub("\n.*", ""))
                    end)
                    return
                end
                verify_recipe_plugin(recipe, plugin, entry)
            end
            return
        end

        if spec.config == nil then
            spec.config = function(plugin, opts)
                local pdir = plugin.dir or dir
                local schemes = detect_plugin_colorschemes(pdir)
                local lua_mod = (type(recipe.module) == "string" and recipe.module ~= "") and recipe.module or detect_plugin_lua_module(pdir, repo)
                if entry then
                    entry.colorschemes = schemes
                    entry.lua_module = lua_mod or ""
                end
                if lua_mod and lua_mod ~= "" then
                    local ok, mod = pcall(require, lua_mod)
                    if not ok then
                        if entry then
                            entry.auto_init = "needs_config"
                            entry.lifecycle = "needs_attention"
                            entry.health_message = "Loading " .. lua_mod .. " failed: " .. tostring(mod):gsub("\n.*", "")
                        end
                        vim.schedule(function()
                            _G.tuim_native_notice(
                                "warning",
                                string.format(
                                    "%s installed, but loading %s failed (%s). Open Plugin Center > Configure to fix.",
                                    plugin.name or repo,
                                    lua_mod,
                                    tostring(mod):gsub("\n.*", "")
                                )
                            )
                        end)
                        return
                    end
                    if type(mod) == "table" and type(mod.setup) == "function" then
                        local setup_ok, err = pcall(mod.setup, type(opts) == "table" and opts or {})
                        if not setup_ok then
                            if entry then
                                entry.auto_init = "needs_config"
                                entry.lifecycle = "needs_attention"
                                entry.health_message = lua_mod .. ".setup() failed: " .. tostring(err):gsub("\n.*", "")
                            end
                            vim.schedule(function()
                                _G.tuim_native_notice(
                                    "warning",
                                    string.format(
                                        "%s installed, but %s.setup() failed (%s). Open Plugin Center > Configure to customize it.",
                                        plugin.name or repo,
                                        lua_mod,
                                        tostring(err):gsub("\n.*", "")
                                    )
                                )
                            end)
                            return
                        end
                        if entry then
                            entry.auto_init = (#schemes > 0) and "colorscheme" or ("recipe(" .. recipe.id .. ")")
                        end
                    elseif entry then
                        entry.auto_init = (#schemes > 0) and "colorscheme" or "runtime"
                    end
                elseif entry then
                    entry.auto_init = (#schemes > 0) and "colorscheme" or "runtime"
                end
                verify_recipe_plugin(recipe, plugin, entry)
            end
        end
        return
    end

    if spec.tuim_auto_setup == false then
        if entry then entry.auto_init = "disabled" end
        return
    end
    if spec.config ~= nil or spec.opts ~= nil or spec.main ~= nil then
        if entry and entry.auto_init ~= "custom" then entry.auto_init = "custom" end
        return
    end
    if spec.lazy == nil and spec.cmd == nil and spec.ft == nil and spec.event == nil and spec.keys == nil then
        spec.lazy = false
    end

    spec.config = function(plugin, opts)
        local pdir = plugin.dir or dir
        local schemes = detect_plugin_colorschemes(pdir)
        local lua_mod = detect_plugin_lua_module(pdir, repo)
        if entry then
            entry.colorschemes = schemes
            entry.lua_module = lua_mod or ""
        end
        if #schemes > 0 then
            -- Colorschemes register via colors/*.lua|vim; only call setup() if opts were passed.
            if lua_mod and type(opts) == "table" and next(opts) ~= nil then
                local ok, mod = pcall(require, lua_mod)
                if ok and type(mod) == "table" and type(mod.setup) == "function" then
                    pcall(mod.setup, opts)
                end
            end
            if entry then
                entry.auto_init = "colorscheme"
                entry.lifecycle = "initialized"
                entry.health_message = "Colorscheme ready (" .. table.concat(schemes, ", ") .. ")"
            end
            return
        end
        if lua_mod then
            local ok, mod = pcall(require, lua_mod)
            if not ok then
                if entry then
                    entry.auto_init = "needs_config"
                    entry.lifecycle = "needs_attention"
                    entry.health_message = "Loading " .. lua_mod .. " failed: " .. tostring(mod):gsub("\n.*", "")
                end
                vim.schedule(function()
                    _G.tuim_native_notice(
                        "warning",
                        string.format(
                            "%s installed, but loading %s failed (%s). Open Extensions > Configure (E) to customize it.",
                            plugin.name or repo,
                            lua_mod,
                            tostring(mod):gsub("\n.*", "")
                        )
                    )
                end)
                return
            end
            if type(mod) == "table" and type(mod.setup) == "function" then
                local setup_ok, err = pcall(mod.setup, type(opts) == "table" and opts or {})
                if setup_ok then
                    if entry then
                        entry.auto_init = "setup(" .. lua_mod .. ")"
                        entry.lifecycle = "initialized"
                        entry.health_message = "Initialized via " .. lua_mod .. ".setup()"
                    end
                else
                    if entry then
                        entry.auto_init = "needs_config"
                        entry.lifecycle = "needs_attention"
                        entry.health_message = lua_mod .. ".setup() failed: " .. tostring(err):gsub("\n.*", "")
                    end
                    vim.schedule(function()
                        _G.tuim_native_notice(
                            "warning",
                            string.format(
                                "%s installed, but default %s.setup() failed (%s). Open Extensions > Configure (E) to customize it.",
                                plugin.name or repo,
                                lua_mod,
                                tostring(err):gsub("\n.*", "")
                            )
                        )
                    end)
                end
                return
            end
        end
        if entry then
            entry.auto_init = "runtime"
            entry.lifecycle = "initialized"
            entry.health_message = "Loaded on runtimepath"
        end
    end
end

local configured_specs = {}
local function configure_spec(spec)
    if type(spec) ~= "table" or configured_specs[spec] then return end
    configured_specs[spec] = true
    local repo = spec[1]
    if type(repo) ~= "string" then return end
    local entry = plugin_inventory[repo]
    if plugin_states[repo] == "removed" then spec.enabled = false; return
    elseif plugin_states[repo] == "disabled" then spec.cond = false; return end

    local config_path = resolve_plugin_config_path(repo)
    if not plugins_disabled and config_path then
        consumed_user_files[config_path] = true
        if entry then
            entry.config_path = config_path
            entry.configured = true
        end
        local chunk, load_err = loadfile(config_path)
        if not chunk then
            if entry then
                entry.config_error = true
                entry.lifecycle = "needs_attention"
                entry.health_message = "Config syntax error: " .. tostring(load_err):gsub("\n.*", "")
            end
            _G.tuim_native_notice("error", "Plugin config syntax error for " .. repo .. ": " .. tostring(load_err))
        else
            local ok, custom = pcall(chunk)
            if ok and (type(custom) == "table" or type(custom) == "function") then
                merge_custom_spec(spec, custom, "Override", repo)
                if entry then entry.auto_init = "custom" end
            elseif ok and custom == nil then
                if entry then
                    entry.config_error = true
                    entry.lifecycle = "needs_attention"
                    entry.health_message = "Plugin config returned nil; expected spec table or function"
                end
                _G.tuim_native_notice(
                    "warning",
                    "Plugin config for " .. repo .. " returned nil; return a lazy.nvim spec table (e.g. `return { opts = {} }`) or a setup function. Put general Lua code in " .. user_config_dir .. "/init.lua."
                )
            else
                if entry then
                    entry.config_error = true
                    entry.lifecycle = "needs_attention"
                    entry.health_message = "Config error: " .. tostring(custom):gsub("\n.*", "")
                end
                _G.tuim_native_notice("error", "Plugin config failed for " .. repo .. ": " .. tostring(custom))
            end
        end
    end

    for _, dep in ipairs(spec.dependencies or {}) do configure_spec(dep) end
    apply_smart_default_init(spec, entry)
end

-- Also support drop-in Lazy spec files in ~/.config/tuim/plugins/*.lua (LazyVim style).
if not plugins_disabled then
    local user_plugins_dir = user_config_dir .. "/plugins"
    for _, item in ipairs(list_dir_entries(user_plugins_dir)) do
        if item.name:sub(-4) == ".lua" then
            local full = user_plugins_dir .. "/" .. item.name
            -- Defer <owner>_<repo>.lua files belonging to already-known plugins to configure_spec.
            local matched_known = false
            for repo in pairs(plugin_inventory) do
                if (repo:gsub("/", "_") .. ".lua") == item.name then
                    matched_known = true
                    break
                end
            end
            if not matched_known then
                local chunk, load_err = loadfile(full)
                if not chunk then
                    _G.tuim_native_notice("error", "Syntax error in " .. full .. ": " .. tostring(load_err))
                else
                    local ok, result = pcall(chunk)
                    if ok and result ~= nil then
                        consumed_user_files[full] = true
                        ingest_user_spec_list(result, full)
                    elseif not ok then
                        _G.tuim_native_notice("error", "Error in " .. full .. ": " .. tostring(result))
                    end
                end
            end
        end
    end
end

for _, spec in ipairs(plugins_setup) do configure_spec(spec) end
_G.tuim_plugin_specs = plugins_setup

local function compute_entry_lifecycle(entry, on_disk)
    if entry.removed then
        return on_disk and "pending_restart" or "not_installed",
            on_disk and "Marked for removal; restart Neovim to clean up" or "Not installed"
    end
    if not on_disk then
        return entry.enabled and "pending_restart" or "not_installed",
            entry.enabled and "Pending install on restart" or "Not installed"
    end
    if entry.config_error or entry.verify_failed or entry.auto_init == "needs_config" or (type(entry.missing_requirements) == "table" and #entry.missing_requirements > 0) then
        local msg = entry.health_message ~= "" and entry.health_message or "Needs configuration or missing requirement"
        return "needs_attention", msg
    end
    if not entry.enabled then
        return "downloaded", "Disabled"
    end
    if entry.verified then
        return "verified", entry.health_message ~= "" and entry.health_message or "Verified"
    end
    if entry.loaded or (entry.auto_init and entry.auto_init ~= "pending" and entry.auto_init ~= "bundled" and entry.auto_init ~= "disabled") then
        return "initialized", entry.health_message ~= "" and entry.health_message or "Initialized"
    end
    return "dependencies_ready", entry.health_message ~= "" and entry.health_message or "Ready (lazy-loaded)"
end

_G.tuim_write_plugin_inventory = function()
    lazy_lock = read_plugin_json("lazy-lock.json", lazy_lock)
    local ok, config = pcall(require, "lazy.core.config")
    if ok then
        for name, plugin in pairs(config.plugins or {}) do
            local repo = plugin[1]
            if type(repo) == "string" then
                if not plugin_inventory[repo] then
                    local recipe = recipes_by_repo[repo]
                    plugin_inventory[repo] = {
                        name = name,
                        full_name = repo,
                        source = "Dependency",
                        required_by = {},
                        enabled = plugin_states[repo] ~= "disabled",
                        removed = plugin_states[repo] == "removed",
                        auto_init = "runtime",
                        config_path = "",
                        colorschemes = {},
                        lua_module = (recipe and recipe.module) or "",
                        supported = recipe ~= nil,
                        recipe_id = (recipe and recipe.id) or "",
                        lifecycle = "downloaded",
                        health_message = "",
                        installed_commit = "",
                        pinned_commit = "",
                        pin_source = "",
                        configured = false,
                        verified = false,
                        config_error = false,
                        missing_requirements = recipe and check_recipe_requirements(recipe) or {},
                    }
                end
                local entry = plugin_inventory[repo]
                entry.name = name
                entry.loaded = (plugin._ and plugin._.loaded ~= nil) or entry.loaded or false
                if plugin.dir and vim.uv.fs_stat(plugin.dir) then
                    entry.colorschemes = detect_plugin_colorschemes(plugin.dir)
                    entry.lua_module = detect_plugin_lua_module(plugin.dir, repo) or entry.lua_module or ""
                end
                if plugin._ and type(plugin._.commit) == "string" and plugin._.commit ~= "" then
                    entry.installed_commit = plugin._.commit
                elseif type(lazy_lock[name]) == "table" and type(lazy_lock[name].commit) == "string" then
                    entry.installed_commit = lazy_lock[name].commit
                end
                entry.config_path = entry.config_path ~= "" and entry.config_path or (resolve_plugin_config_path(repo) or "")
            end
        end
        for _, plugin in pairs(config.plugins or {}) do
            for _, name in ipairs(plugin.dependencies or {}) do
                local dep = config.plugins[name]
                local entry = dep and plugin_inventory[dep[1]]
                if entry and type(plugin[1]) == "string" and not vim.tbl_contains(entry.required_by, plugin[1]) then
                    table.insert(entry.required_by, plugin[1])
                end
            end
        end
    end
    local entries = {}
    for repo, entry in pairs(plugin_inventory) do
        local on_disk = vim.uv.fs_stat(lazy_root .. "/" .. entry.name) ~= nil
        if entry.installed_commit == "" and type(lazy_lock[entry.name]) == "table" and type(lazy_lock[entry.name].commit) == "string" then
            entry.installed_commit = lazy_lock[entry.name].commit
        end
        local saved_opts = type(recipe_options[repo]) == "table" and recipe_options[repo] or nil
        entry.configured = (entry.config_path ~= nil and entry.config_path ~= "") or (saved_opts ~= nil and next(saved_opts) ~= nil)
        local lc, hmsg = compute_entry_lifecycle(entry, on_disk)
        entry.lifecycle = lc
        entry.health_message = hmsg
        table.insert(entries, entry)
    end
    table.sort(entries, function(a, b) return a.name:lower() < b.name:lower() end)
    vim.fn.mkdir(plugin_data, "p")
    local path = plugin_data .. "/plugin_inventory.json"
    local tmp = path .. "." .. vim.fn.getpid() .. ".tmp"
    local file = io.open(tmp, "w")
    if file then
        file:write(vim.json.encode(entries)); file:close(); os.rename(tmp, path)
    end
end

_G.tuim_verify_plugin = function(repo)
    local entry = plugin_inventory[repo]
    local recipe = recipes_by_repo[repo]
    if plugins_disabled then
        return { ok = false, lifecycle = "downloaded", health_message = "Plugins are disabled for this session" }
    end
    if not entry then
        return { ok = false, lifecycle = "not_installed", health_message = "Plugin is not registered" }
    end
    local pdir = lazy_root .. "/" .. entry.name
    local on_disk = vim.uv.fs_stat(pdir) ~= nil
    if entry.removed or not on_disk then
        _G.tuim_write_plugin_inventory()
        return { ok = false, lifecycle = entry.lifecycle, health_message = entry.health_message }
    end
    if not entry.enabled then
        _G.tuim_write_plugin_inventory()
        return { ok = false, lifecycle = "downloaded", health_message = "Plugin is disabled; enable and restart to verify" }
    end
    if entry.config_error or entry.auto_init == "needs_config" then
        -- Never allow symbol presence to mask a prior config or setup failure.
        entry.verified = false
        entry.verify_failed = true
        _G.tuim_write_plugin_inventory()
        return { ok = false, lifecycle = "needs_attention", health_message = entry.health_message }
    end
    local is_loaded = entry.loaded == true
    if not is_loaded then
        local ok_cfg, cfg = pcall(require, "lazy.core.config")
        if ok_cfg and type(cfg.plugins) == "table" then
            local p = cfg.plugins[entry.name]
            if p and p._ and p._.loaded ~= nil then
                is_loaded = true
                entry.loaded = true
            end
        end
    end
    if not is_loaded then
        -- Do not call require() on an uninitialized/lazy plugin (avoids autoload side effects).
        if recipe then
            entry.missing_requirements = check_recipe_requirements(recipe)
        end
        _G.tuim_write_plugin_inventory()
        return { ok = false, lifecycle = entry.lifecycle, health_message = entry.health_message }
    end
    if recipe then
        verify_recipe_plugin(recipe, { dir = pdir }, entry)
    end
    _G.tuim_write_plugin_inventory()
    return {
        ok = entry.lifecycle == "verified" or entry.lifecycle == "initialized",
        lifecycle = entry.lifecycle,
        health_message = entry.health_message,
    }
end

_G.tuim_write_plugin_inventory()
vim.api.nvim_create_autocmd("User", {
    pattern = { "LazyDone", "LazyLoad", "LazyInstall", "LazyClean", "LazySync" },
    callback = function() vim.schedule(_G.tuim_write_plugin_inventory) end,
})
-- Clean only plugins explicitly uninstalled by the user, after the new spec loads.
vim.api.nvim_create_autocmd("VimEnter", { once = true, callback = function()
    if plugins_disabled then return end
    local names = {}
    for repo, entry in pairs(plugin_inventory) do
        if plugin_states[repo] == "removed" and repo ~= "folke/lazy.nvim" then table.insert(names, entry.name) end
    end
    if #names > 0 then
        vim.schedule(function()
            local ok, lazy = pcall(require, "lazy")
            if ok then
                local targets = {}
                for _, plugin in ipairs(require("lazy.core.config").to_clean or {}) do
                    if vim.tbl_contains(names, plugin.name) then table.insert(targets, plugin) end
                end
                if #targets > 0 then lazy.clean({ plugins = targets, show = false }) end
            end
        end)
    end
end })
local lazy_opts = {
    root = lazy_root,
    lockfile = plugin_data .. "/lazy-lock.json",
    install = {
        missing = true,
        colorscheme = { active_theme_name, "vscode", "habamax" },
    },
    change_detection = {
        enabled = false,
        notify = false,
    },
    checker = {
        enabled = false,
    },
    ui = {
        border = "rounded",
    },
    performance = {
        rtp = {
            reset = false, -- Prevent lazy.nvim from adding user's ~/.config/nvim back to RTP
        },
    },
}
local lazy_setup_done = false
if not plugins_disabled then
    local lazy_ok, lazy = pcall(require, "lazy")
    if lazy_ok then
      local setup_ok, setup_err = pcall(lazy.setup, plugins_setup, lazy_opts)
      if not setup_ok then
        plugins_disabled = true
        pending_plugin_notice = "Plugin setup failed; Tuim continued without plugins. Use TUIM_DISABLE_PLUGINS=1 to repair safely."
        vim.schedule(function() vim.notify(tostring(setup_err), vim.log.levels.ERROR) end)
      else
        lazy_setup_done = true
        pcall(os.remove, plugin_data .. "/plugin_pending_restart.json")
        _G.tuim_write_plugin_inventory()
      end
    else
      plugins_disabled = true
      pending_plugin_notice = "lazy.nvim is unavailable; Tuim continued offline. Retry plugin sync from Settings > Plugins."
    end
end

if plugins_disabled then
  vim.g.tuim_plugins_disabled = true
end

_G.tuim_retry_plugins = function()
  if os.getenv("TUIM_DISABLE_PLUGINS") == "1" then
    _G.tuim_native_notice("warning", "Recovery mode (TUIM_DISABLE_PLUGINS=1) skips user plugin configs. Restart Gtuim normally to enable plugins.")
    return
  end
  if lazy_disabled_by_setting then
    _G.tuim_native_notice("warning", "Plugins are disabled in Settings > Plugins. Re-enable Lazy.nvim and restart Neovim to load plugins.")
    return
  end
  if lazy_setup_done then
    _G.tuim_native_notice("info", "Synchronizing plugins with Lazy...")
    vim.cmd("Lazy sync")
    return
  end
  _G.tuim_native_notice("info", "Retrying plugin bootstrap and synchronization...")
  if not vim.uv.fs_stat(lazypath) then
    local cloned, clone_err = clone_lazy_bounded(lazypath, 15000)
    if not cloned then
      _G.tuim_native_notice("error", "Plugin retry failed (" .. tostring(clone_err) .. "). Check git/network access, or continue offline.")
      return
    end
  end
  vim.opt.rtp:prepend(lazypath)
  local lazy_ok, lazy = pcall(require, "lazy")
  local setup_ok, setup_err = false, nil
  if lazy_ok then
    setup_ok, setup_err = pcall(lazy.setup, plugins_setup, lazy_opts)
  end
  if not lazy_ok or not setup_ok then
    _G.tuim_native_notice("error", "Plugin setup retry failed; continue offline or use TUIM_DISABLE_PLUGINS=1.")
    if setup_err then vim.notify(tostring(setup_err), vim.log.levels.ERROR) end
    return
  end
  lazy_setup_done = true
  plugins_disabled = false
  vim.g.tuim_plugins_disabled = false
  _G.tuim_native_notice("info", "Plugin manager recovered; synchronizing plugins.")
  vim.cmd("Lazy sync")
end

if pending_plugin_notice then
  vim.defer_fn(function() _G.tuim_native_notice("warning", pending_plugin_notice) end, 100)
elseif plugin_bootstrapped then
  vim.defer_fn(function() _G.tuim_native_notice("info", "Plugin manager installed; bundled plugins are synchronizing.") end, 100)
elseif os.getenv("TUIM_DISABLE_PLUGINS") == "1" then
  vim.defer_fn(function() _G.tuim_native_notice("warning", "Plugins are disabled for this recovery session.") end, 100)
elseif lazy_disabled_by_setting then
  vim.defer_fn(function() _G.tuim_native_notice("info", "Plugins are disabled in Settings > Plugins.") end, 100)
end

local ide_mappings = {
    i = {
        ['<Esc>'] = '<nop>',
        ['<C-Left>'] = '<C-o>b', ['<C-Right>'] = '<C-o>w',
        ['<D-Left>'] = '<Home>', ['<D-Right>'] = '<End>',
        ['<S-Left>'] = '<C-o>v<Left>', ['<S-Right>'] = '<C-o>v<Right>',
        ['<S-Up>'] = '<C-o>v<Up>', ['<S-Down>'] = '<C-o>v<Down>',
        ['<S-Home>'] = '<C-o>v<Home>', ['<S-End>'] = '<C-o>v<End>',
        ['<C-S-Left>'] = '<C-o>v<C-Left>', ['<C-S-Right>'] = '<C-o>v<C-Right>',
        ['<D-S-Left>'] = '<C-o>v<C-Left>', ['<D-S-Right>'] = '<C-o>v<C-Right>',
        ['<D-S-Home>'] = '<C-o>v<Home>', ['<D-S-End>'] = '<C-o>v<End>',
    },
    n = { ['<Esc>'] = 'i', ['ZZ'] = '<nop>', ['ZQ'] = '<nop>' },
    v = {
        ['<Esc>'] = '<C-c>i',
        ['<S-Left>'] = '<Left>', ['<S-Right>'] = '<Right>',
        ['<S-Up>'] = '<Up>', ['<S-Down>'] = '<Down>',
        ['<S-Home>'] = '<Home>', ['<S-End>'] = '<End>',
        ['<C-S-Left>'] = 'b', ['<C-S-Right>'] = 'w',
        ['<D-S-Left>'] = 'b', ['<D-S-Right>'] = 'w',
        ['<D-S-Home>'] = '<Home>', ['<D-S-End>'] = '<End>',
        ['<BS>'] = '"_c', ['<Del>'] = '"_c',
    },
}

local ide_undo_break_keys = {
    { '<Space>', '<Space><C-g>u', '<Space>' },
    { '.', '.<C-g>u', '.' },
    { ',', ',<C-g>u', ',' },
    { ';', ';<C-g>u', ';' },
    { '!', '!<C-g>u', '!' },
    { '?', '?<C-g>u', '?' },
}
local ide_cr_fallback_mapped = false

local save_prompt_buffer = nil
local confirm_in_progress = false
local cmdline_action_pending = false

local ide_excluded_filetypes = {
    alpha = true,
    dashboard = true,
    tuim_dashboard = true,
    netrw = true,
    help = true,
    qf = true,
    TelescopePrompt = true,
    TelescopeResults = true,
    ['tuim-settings'] = true,
    ['tuim-onboarding'] = true,
    vimbindings = true,
}

local function can_ide_insert(bufnr, winid)
    if not vim.g.tuim_ide_mode or save_prompt_buffer or confirm_in_progress or cmdline_action_pending then
        return false
    end
    winid = winid or vim.api.nvim_get_current_win()
    if not vim.api.nvim_win_is_valid(winid) then return false end
    local ok_cfg, cfg = pcall(vim.api.nvim_win_get_config, winid)
    if not ok_cfg or cfg.relative ~= '' then return false end
    bufnr = bufnr or vim.api.nvim_win_get_buf(winid)
    if not vim.api.nvim_buf_is_valid(bufnr) then return false end
    local bo = vim.bo[bufnr]
    if not bo.modifiable then return false end
    if bo.buftype ~= '' and bo.buftype ~= 'acwrite' then return false end
    if ide_excluded_filetypes[bo.filetype] then return false end
    return true
end

local function ide_startinsert(append_eol)
    if not can_ide_insert() then return end
    vim.schedule(function()
        if not can_ide_insert() then return end
        local mode_info = vim.api.nvim_get_mode()
        if mode_info.blocking then return end
        if mode_info.mode == 'n' then
            pcall(vim.cmd, append_eol and 'startinsert!' or 'startinsert')
        end
    end)
end

local function ide_save_cursor(win, buf)
    win = win or vim.api.nvim_get_current_win()
    if not vim.api.nvim_win_is_valid(win) then return nil end
    buf = buf or vim.api.nvim_win_get_buf(win)
    if not vim.api.nvim_buf_is_valid(buf) then return nil end
    local m = vim.api.nvim_get_mode().mode
    local ok_pos, pos = pcall(vim.api.nvim_win_get_cursor, win)
    if not ok_pos then return nil end
    local line = vim.api.nvim_buf_get_lines(buf, pos[1] - 1, pos[1], false)[1] or ''
    local was_insert = m:sub(1, 1) == 'i'
    return {
        win = win,
        buf = buf,
        row = pos[1],
        col = pos[2],
        was_insert = was_insert,
        at_eol = was_insert and pos[2] >= #line,
    }
end

local function ide_restore_cursor(saved)
    if not saved then
        ide_startinsert()
        return
    end
    vim.schedule(function()
        if not vim.api.nvim_win_is_valid(saved.win) or vim.api.nvim_win_get_buf(saved.win) ~= saved.buf then
            ide_startinsert()
            return
        end
        local line_count = vim.api.nvim_buf_line_count(saved.buf)
        local row = math.max(1, math.min(saved.row, line_count))
        local line = vim.api.nvim_buf_get_lines(saved.buf, row - 1, row, false)[1] or ''
        if saved.was_insert and can_ide_insert(saved.buf, saved.win) then
            if saved.at_eol or saved.col >= #line then
                pcall(vim.api.nvim_win_set_cursor, saved.win, { row, math.max(0, #line - 1) })
                pcall(vim.cmd, 'startinsert!')
            else
                pcall(vim.api.nvim_win_set_cursor, saved.win, { row, math.max(0, saved.col) })
                pcall(vim.cmd, 'startinsert')
            end
        else
            local max_col = math.max(0, #line - 1)
            pcall(vim.api.nvim_win_set_cursor, saved.win, { row, math.max(0, math.min(saved.col, max_col)) })
            ide_startinsert()
        end
    end)
end

local function ide_clipboard_register()
    local system_clipboard_enabled = vim.o.clipboard:match('unnamedplus') ~= nil
    return vim.fn.has('clipboard') == 1 and system_clipboard_enabled and '+' or '"'
end

-- Native shortcuts such as Ctrl+F can be pressed while a Telescope picker or
-- another floating window owns focus.  Plain `:close` refuses to close a
-- modified prompt buffer (E37), and the resulting RPC error is rendered into
-- the UI.  Let Telescope clean itself up first, then force-close any remaining
-- floats so global actions are safe to repeat.
_G.tuim_close_floating_windows = function()
    local telescope_prompts = {}
    for _, winid in ipairs(vim.api.nvim_list_wins()) do
        local ok, bufnr = pcall(vim.api.nvim_win_get_buf, winid)
        if ok and vim.bo[bufnr].filetype == 'TelescopePrompt' then
            telescope_prompts[bufnr] = true
        end
    end

    if next(telescope_prompts) then
        local ok, actions = pcall(require, 'telescope.actions')
        if ok then
            for bufnr in pairs(telescope_prompts) do
                pcall(actions.close, bufnr)
            end
        end
    end

    for _, winid in ipairs(vim.api.nvim_list_wins()) do
        local ok, config = pcall(vim.api.nvim_win_get_config, winid)
        if ok and config.relative ~= '' then
            pcall(vim.api.nvim_win_close, winid, true)
        end
    end
end

local function tuim_primary_editor_win()
    local best_win, best_col, best_row = nil, math.huge, math.huge
    for _, winid in ipairs(vim.api.nvim_list_wins()) do
        local config = vim.api.nvim_win_get_config(winid)
        local buf = vim.api.nvim_win_get_buf(winid)
        if config.relative == "" and vim.bo[buf].buftype == "" then
            local pos = vim.api.nvim_win_get_position(winid)
            if pos[2] < best_col or (pos[2] == best_col and pos[1] < best_row) then
                best_win, best_col, best_row = winid, pos[2], pos[1]
            end
        end
    end
    return best_win
end

_G.tuim_select_buffer = function(bufnr)
    bufnr = tonumber(bufnr)
    if not bufnr or not vim.api.nvim_buf_is_valid(bufnr) then return false end
    for _, winid in ipairs(vim.fn.win_findbuf(bufnr)) do
        if vim.api.nvim_win_is_valid(winid) then
            vim.api.nvim_set_current_win(winid)
            return true
        end
    end
    local primary = tuim_primary_editor_win()
    if primary and vim.api.nvim_win_is_valid(primary) then vim.api.nvim_set_current_win(primary) end
    vim.api.nvim_set_current_buf(bufnr)
    return true
end

_G.tuim_new_primary_buffer = function()
    local primary = tuim_primary_editor_win()
    if primary and vim.api.nvim_win_is_valid(primary) then vim.api.nvim_set_current_win(primary) end
    vim.cmd("enew")
end

local function tuim_find_safe_unnamed_name(used_names, counter_ref)
    while true do
        local candidate = (counter_ref.value == 0) and 'Untitled' or string.format('Untitled-%d', counter_ref.value)
        counter_ref.value = counter_ref.value + 1
        if not vim.uv.fs_stat(candidate) and not used_names[candidate] then
            used_names[candidate] = true
            return candidate
        end
    end
end

local quit_in_progress = false
local tuim_save_buffer_impl = nil

local function clear_prompt_cmdline()
    pcall(vim.api.nvim_echo, { { '' } }, false, {})
    pcall(vim.cmd, 'redraw')
end

local function buffer_display_label(bufnr)
    local name = vim.api.nvim_buf_get_name(bufnr)
    if name == '' then return '[No Name]' end
    return vim.fn.fnamemodify(name, ':~:.')
end

-- Quit Tuim with confirmation of unsaved changes.
-- Never renames unnamed buffers to ./Untitled; asks for a path on Save,
-- preserves unnamed identity on Cancel, and avoids hit-enter stalls.
_G.tuim_confirm_quit = function()
    if quit_in_progress then return end
    quit_in_progress = true
    local saved_cursor = ide_save_cursor()
    local aborted = false
    local deferred_unnamed = nil

    local dirty_bufs = {}
    for _, b in ipairs(vim.api.nvim_list_bufs()) do
        if vim.api.nvim_buf_is_valid(b) and vim.bo[b].buflisted and vim.bo[b].modified and vim.bo[b].buftype == '' then
            table.insert(dirty_bufs, b)
        end
    end

    if #dirty_bufs == 0 then
        quit_in_progress = false
        pcall(vim.cmd, 'qa')
        return
    end

    confirm_in_progress = true
    pcall(vim.cmd, 'stopinsert')
    for _, b in ipairs(dirty_bufs) do
        if vim.api.nvim_buf_is_valid(b) and vim.bo[b].modified then
            if vim.api.nvim_get_current_buf() ~= b then _G.tuim_select_buffer(b) end
            local label = buffer_display_label(b)
            local ok_c, choice = pcall(
                vim.fn.confirm,
                string.format('Save changes to "%s"?', label),
                '&Yes\n&No\n&Cancel',
                3,
                'Question'
            )
            clear_prompt_cmdline()
            if not ok_c or choice == 0 or choice == 3 then
                aborted = true
                break
            elseif choice == 1 then
                if vim.api.nvim_buf_get_name(b) == '' then
                    deferred_unnamed = b
                    aborted = true
                    break
                else
                    local ok_w, err_w = pcall(vim.api.nvim_buf_call, b, function()
                        vim.cmd('silent write')
                    end)
                    if ok_w then
                        local saved_name = vim.api.nvim_buf_get_name(b)
                        _G.tuim_native_notice('info', 'Saved ' .. vim.fn.fnamemodify(saved_name, ':~:.'))
                        pcall(vim.rpcnotify, 1, 'tuim_file_saved', saved_name)
                    else
                        _G.tuim_native_notice('error', _G.tuim_save_error_message(tostring(err_w), vim.api.nvim_buf_get_name(b)))
                        aborted = true
                        break
                    end
                end
            end
        end
    end
    confirm_in_progress = false
    quit_in_progress = false

    if deferred_unnamed and vim.api.nvim_buf_is_valid(deferred_unnamed) and tuim_save_buffer_impl then
        vim.schedule(function()
            tuim_save_buffer_impl(deferred_unnamed, { quit_after = true, saved_cursor = saved_cursor })
        end)
        return
    end

    if aborted then
        if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
        if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end
        ide_restore_cursor(saved_cursor)
        return
    end

    pcall(vim.cmd, 'qa!')
end

-- Close a specific buffer from Tuim's tab strip or Ctrl+W.
-- Uses silent write + explicit buffer deletion to avoid Neovim's hit-enter
-- stall, never renames unnamed buffers to ./Untitled, and prompts for a path
-- when saving an unnamed dirty buffer.
_G.tuim_close_buffer = function(bufnr)
    bufnr = tonumber(bufnr)
    if not bufnr or not vim.api.nvim_buf_is_valid(bufnr) then return false end

    -- AI terminals belong to their split rather than the file tab strip. Stop
    -- their job and remove them without Neovim's "add ! to override" prompt.
    if bufnr == _G.last_ai_buf and _G.CloseAITerminal then
        _G.CloseAITerminal()
        return not vim.api.nvim_buf_is_valid(bufnr)
    end

    if vim.bo[bufnr].buftype == 'terminal' then
        local windows = vim.fn.win_findbuf(bufnr)
        -- Closing a terminal buffer explicitly ends its job. Delete first so
        -- Neovim can retain a usable buffer when this is the last window.
        vim.api.nvim_buf_delete(bufnr, { force = true })
        for _, win in ipairs(windows) do
            if vim.api.nvim_win_is_valid(win) and #vim.api.nvim_list_wins() > 1 then
                pcall(vim.api.nvim_win_close, win, false)
            end
        end
        if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
        if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end
        return not vim.api.nvim_buf_is_valid(bufnr)
    end

    if vim.bo[bufnr].modified then
        if vim.api.nvim_get_current_buf() ~= bufnr then _G.tuim_select_buffer(bufnr) end
        local saved_cursor = ide_save_cursor()
        local was_unnamed = vim.api.nvim_buf_get_name(bufnr) == '' and vim.bo[bufnr].buftype == ''
        local label = buffer_display_label(bufnr)
        confirm_in_progress = true
        pcall(vim.cmd, 'stopinsert')
        local ok_c, choice = pcall(
            vim.fn.confirm,
            string.format('Save changes to "%s"?', label),
            '&Yes\n&No\n&Cancel',
            3,
            'Question'
        )
        confirm_in_progress = false
        clear_prompt_cmdline()

        if not ok_c or choice == 0 or choice == 3 then
            if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
            if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end
            ide_restore_cursor(saved_cursor)
            return false
        elseif choice == 2 then
            pcall(vim.api.nvim_buf_delete, bufnr, { force = true })
            if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
            if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end
            if _G.tuim_notify_cursor then _G.tuim_notify_cursor() end
            ide_startinsert()
            return not vim.api.nvim_buf_is_valid(bufnr)
        elseif choice == 1 then
            if was_unnamed then
                -- Defer filename prompt until the async QuitConfirmation request
                -- completes so Esc cancels vim.ui.input cleanly instead of sending 'c'.
                vim.schedule(function()
                    if vim.api.nvim_buf_is_valid(bufnr) and tuim_save_buffer_impl then
                        tuim_save_buffer_impl(bufnr, { close_after = true, saved_cursor = saved_cursor })
                    end
                end)
                return false
            end
            local ok_w, err_w = pcall(vim.api.nvim_buf_call, bufnr, function()
                vim.cmd('silent write')
            end)
            if ok_w then
                local saved_name = vim.api.nvim_buf_get_name(bufnr)
                _G.tuim_native_notice('info', 'Saved ' .. vim.fn.fnamemodify(saved_name, ':~:.'))
                pcall(vim.rpcnotify, 1, 'tuim_file_saved', saved_name)
                pcall(vim.api.nvim_buf_delete, bufnr, {})
                if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
                if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end
                if _G.tuim_notify_cursor then _G.tuim_notify_cursor() end
                ide_startinsert()
                return not vim.api.nvim_buf_is_valid(bufnr)
            else
                _G.tuim_native_notice('error', _G.tuim_save_error_message(tostring(err_w), vim.api.nvim_buf_get_name(bufnr)))
                if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
                ide_restore_cursor(saved_cursor)
                return false
            end
        end
    else
        pcall(vim.api.nvim_buf_delete, bufnr, {})
        if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
        if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end
        if _G.tuim_notify_cursor then _G.tuim_notify_cursor() end
        ide_startinsert()
    end
    return not vim.api.nvim_buf_is_valid(bufnr)
end

local function list_normal_splits()
    local splits = {}
    for _, w in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
        local cfg = vim.api.nvim_win_get_config(w)
        if cfg.relative == "" and not cfg.external then
            table.insert(splits, w)
        end
    end
    return splits
end

_G.tuim_focus_split = function(winid)
    winid = tonumber(winid)
    if not winid or not vim.api.nvim_win_is_valid(winid) then return false end
    local ok = pcall(vim.api.nvim_set_current_win, winid)
    if ok and _G.tuim_notify_win_positions then
        vim.schedule(_G.tuim_notify_win_positions)
    end
    return ok
end

_G.tuim_close_split = function(winid, bufnr)
    winid = tonumber(winid)
    if not winid or winid == 0 then winid = vim.api.nvim_get_current_win() end
    if not winid or not vim.api.nvim_win_is_valid(winid) then return false end
    bufnr = tonumber(bufnr)
    if not bufnr or bufnr == 0 then bufnr = vim.api.nvim_win_get_buf(winid) end
    if bufnr == _G.last_ai_buf and _G.CloseAITerminal then
        _G.CloseAITerminal()
        if _G.tuim_notify_win_positions then vim.schedule(_G.tuim_notify_win_positions) end
        return true
    end
    if #list_normal_splits() <= 1 then
        _G.tuim_native_notice("info", "Only one split is open.")
        return false
    end
    local ok, err = pcall(vim.api.nvim_win_close, winid, false)
    if not ok then _G.tuim_native_notice("warning", tostring(err)) end
    if _G.tuim_notify_win_positions then vim.schedule(_G.tuim_notify_win_positions) end
    return ok
end

_G.tuim_only_split = function(winid)
    winid = tonumber(winid)
    if not winid or winid == 0 then winid = vim.api.nvim_get_current_win() end
    if not winid or not vim.api.nvim_win_is_valid(winid) then return false end
    pcall(vim.api.nvim_set_current_win, winid)
    for _, w in ipairs(list_normal_splits()) do
        if w ~= winid and vim.api.nvim_win_is_valid(w) then
            local b = vim.api.nvim_win_get_buf(w)
            if b == _G.last_ai_buf and _G.CloseAITerminal then
                _G.CloseAITerminal()
            else
                pcall(vim.api.nvim_win_close, w, false)
            end
        end
    end
    if vim.api.nvim_win_is_valid(winid) then
        pcall(vim.api.nvim_set_current_win, winid)
    end
    if _G.tuim_notify_win_positions then vim.schedule(_G.tuim_notify_win_positions) end
    return true
end

local function is_normal_split(winid)
    if not winid or not vim.api.nvim_win_is_valid(winid) then return false end
    local cfg = vim.api.nvim_win_get_config(winid)
    return cfg.relative == "" and not cfg.external
end

_G.tuim_move_split = function(src_win, action, target_win)
    src_win = tonumber(src_win)
    target_win = tonumber(target_win) or 0
    if not is_normal_split(src_win) then return false end
    local splits = list_normal_splits()
    if #splits <= 1 then return false end

    local function notify_all()
        if _G.tuim_notify_win_positions then vim.schedule(_G.tuim_notify_win_positions) end
        if _G.tuim_notify_buffers then vim.schedule(_G.tuim_notify_buffers) end
    end

    local dock_map = {
        dock_left = "H",
        dock_right = "L",
        dock_top = "K",
        dock_bottom = "J",
    }

    local dock_cmd = dock_map[action]
    if not dock_cmd and #splits == 2 then
        local two_map = {
            left = "H",
            right = "L",
            above = "K",
            below = "J",
        }
        dock_cmd = two_map[action]
    end

    if dock_cmd then
        pcall(vim.api.nvim_set_current_win, src_win)
        pcall(vim.cmd, "wincmd " .. dock_cmd)
        pcall(vim.cmd, "wincmd =")
        if vim.api.nvim_win_is_valid(src_win) then
            pcall(vim.api.nvim_set_current_win, src_win)
        end
        notify_all()
        return true
    end

    if not is_normal_split(target_win) or src_win == target_win then
        return false
    end

    if action == "swap" then
        local src_buf = vim.api.nvim_win_get_buf(src_win)
        local tgt_buf = vim.api.nvim_win_get_buf(target_win)
        local src_view = vim.api.nvim_win_call(src_win, vim.fn.winsaveview)
        local tgt_view = vim.api.nvim_win_call(target_win, vim.fn.winsaveview)
        pcall(vim.api.nvim_win_set_buf, src_win, tgt_buf)
        pcall(vim.api.nvim_win_call, src_win, function() vim.fn.winrestview(tgt_view) end)
        pcall(vim.api.nvim_win_set_buf, target_win, src_buf)
        pcall(vim.api.nvim_win_call, target_win, function() vim.fn.winrestview(src_view) end)
        pcall(vim.api.nvim_set_current_win, target_win)
        notify_all()
        return true
    end

    if action == "above" or action == "below" or action == "left" or action == "right" then
        local src_buf = vim.api.nvim_win_get_buf(src_win)
        local src_view = vim.api.nvim_win_call(src_win, vim.fn.winsaveview)
        local saved_opts = {
            wrap = vim.wo[src_win].wrap,
            number = vim.wo[src_win].number,
            relativenumber = vim.wo[src_win].relativenumber,
            signcolumn = vim.wo[src_win].signcolumn,
            foldenable = vim.wo[src_win].foldenable,
        }
        local ok, new_win = pcall(vim.api.nvim_open_win, src_buf, true, {
            win = target_win,
            split = action,
        })
        if not ok or not new_win or not vim.api.nvim_win_is_valid(new_win) then
            pcall(vim.api.nvim_set_current_win, target_win)
            local cmd = ({
                above = "leftabove split",
                below = "rightbelow split",
                left = "leftabove vsplit",
                right = "rightbelow vsplit",
            })[action]
            pcall(vim.cmd, cmd)
            new_win = vim.api.nvim_get_current_win()
            pcall(vim.api.nvim_win_set_buf, new_win, src_buf)
        end
        if vim.api.nvim_win_is_valid(new_win) then
            for opt, val in pairs(saved_opts) do
                pcall(function() vim.wo[new_win][opt] = val end)
            end
            pcall(vim.api.nvim_win_call, new_win, function() vim.fn.winrestview(src_view) end)
            if vim.api.nvim_win_is_valid(src_win) and src_win ~= new_win then
                pcall(vim.api.nvim_win_close, src_win, true)
            end
            pcall(vim.api.nvim_set_current_win, new_win)
            pcall(vim.cmd, "wincmd =")
            if vim.api.nvim_win_is_valid(new_win) then
                pcall(vim.api.nvim_set_current_win, new_win)
            end
        end
        notify_all()
        return true
    end

    return false
end

_G.tuim_rename_path = function(old_path, new_path)
    if type(old_path) ~= "string" or type(new_path) ~= "string" then return false end
    local old_norm = vim.fs.normalize(old_path)
    local new_norm = vim.fs.normalize(new_path)
    local prefix = old_norm .. "/"
    local updated = false
    for _, b in ipairs(vim.api.nvim_list_bufs()) do
        if vim.api.nvim_buf_is_valid(b) then
            local name = vim.api.nvim_buf_get_name(b)
            if name ~= "" then
                local norm = vim.fs.normalize(name)
                if norm == old_norm then
                    pcall(vim.api.nvim_buf_set_name, b, new_norm)
                    updated = true
                elseif norm:sub(1, #prefix) == prefix then
                    pcall(vim.api.nvim_buf_set_name, b, new_norm .. norm:sub(#old_norm + 1))
                    updated = true
                end
            end
        end
    end
    if updated then
        if _G.tuim_notify_buffers then vim.schedule(_G.tuim_notify_buffers) end
        if _G.tuim_notify_win_positions then vim.schedule(_G.tuim_notify_win_positions) end
    end
    return updated
end

function _G.TuimClickCloseSplit(_, _, button)
    if button ~= "l" then return end
    local win = vim.fn.getmousepos().winid
    vim.schedule(function()
        if vim.api.nvim_win_is_valid(win) then
            _G.tuim_close_split(win, vim.api.nvim_win_get_buf(win))
        end
    end)
end

-- Turn a raw :write error (with Lua traceback) into one readable line.
_G.tuim_save_error_message = function(err, target)
    if err:match('E45:') or err:match('E505:') then return 'Cannot save: file is read-only' end
    local dir = vim.fn.fnamemodify(target, ':h')
    if err:match('E212:') and dir ~= '' and not vim.uv.fs_stat(dir) then
        return 'Cannot save: directory ' .. vim.fn.fnamemodify(dir, ':~:.') .. '/ does not exist'
    end
    return 'Cannot save: ' .. (err:match('(E%d+:[^\n]*)') or err:match('^[^\n]*'))
end

-- All native save and Save As actions share safe naming, error reporting,
-- cursor restoration, and immediate buffer/title/explorer notifications.
tuim_save_buffer_impl = function(bufnr, opts)
    if save_prompt_buffer then return end
    opts = opts or {}
    bufnr = tonumber(bufnr) or vim.api.nvim_get_current_buf()
    if not vim.api.nvim_buf_is_valid(bufnr) then return end

    local saved_cursor = opts.saved_cursor or ide_save_cursor()
    local force_prompt = opts.force_prompt == true

    local function write(path, is_save_as)
        if not vim.api.nvim_buf_is_valid(bufnr) then return false end
        local orig_name = vim.api.nvim_buf_get_name(bufnr)
        local was_unnamed = orig_name == ''
        local cmd
        if is_save_as and path then
            cmd = 'silent saveas ' .. vim.fn.fnameescape(path)
        elseif path then
            cmd = 'silent write ' .. vim.fn.fnameescape(path)
        else
            cmd = 'silent write'
        end
        local ok, err = pcall(vim.api.nvim_buf_call, bufnr, function()
            vim.cmd(cmd)
        end)
        if ok then
            local saved_name = vim.api.nvim_buf_get_name(bufnr)
            if is_save_as and orig_name ~= '' and orig_name ~= saved_name then
                -- :saveas creates a new unloaded alternate buffer for the old name;
                -- remove it so Save As cleanly renames the active tab without a ghost tab.
                local orig_norm = vim.fs.normalize(orig_name)
                for _, b in ipairs(vim.api.nvim_list_bufs()) do
                    if b ~= bufnr and vim.api.nvim_buf_is_valid(b) and not vim.api.nvim_buf_is_loaded(b) then
                        local bname = vim.api.nvim_buf_get_name(b)
                        if bname ~= '' and vim.fs.normalize(bname) == orig_norm then
                            pcall(vim.api.nvim_buf_delete, b, { force = true })
                        end
                    end
                end
            end
            if was_unnamed or is_save_as then
                pcall(vim.api.nvim_buf_call, bufnr, function()
                    vim.cmd('silent! filetype detect')
                end)
            end
            local rel = vim.fn.fnamemodify(saved_name, ':~:.')
            _G.tuim_native_notice('info', (is_save_as and 'Saved as ' or 'Saved ') .. rel)
            if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
            if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end
            if _G.tuim_notify_cursor then _G.tuim_notify_cursor() end
            pcall(vim.rpcnotify, 1, 'tuim_file_saved', saved_name)
            if opts.close_after then
                pcall(vim.api.nvim_buf_delete, bufnr, {})
                if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
                if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end
                if _G.tuim_notify_cursor then _G.tuim_notify_cursor() end
                ide_startinsert()
            elseif opts.quit_after then
                vim.schedule(function()
                    if _G.tuim_confirm_quit then _G.tuim_confirm_quit() end
                end)
            else
                ide_restore_cursor(saved_cursor)
            end
            return true
        else
            -- A failed :write/:saveas may assign the name before discovering an I/O error.
            -- Restore the previous name so the user can retry cleanly.
            local target = path or vim.api.nvim_buf_get_name(bufnr)
            if was_unnamed then
                pcall(vim.api.nvim_buf_set_name, bufnr, '')
            elseif is_save_as and vim.api.nvim_buf_get_name(bufnr) ~= orig_name then
                pcall(vim.api.nvim_buf_set_name, bufnr, orig_name)
            end
            _G.tuim_native_notice('error', _G.tuim_save_error_message(tostring(err), target))
            if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
            ide_restore_cursor(saved_cursor)
            return false
        end
    end

    if not force_prompt and (vim.api.nvim_buf_get_name(bufnr) ~= '' or vim.bo[bufnr].buftype ~= '') then
        write(nil, false)
        return
    end
    if force_prompt and vim.bo[bufnr].buftype ~= '' then
        _G.tuim_native_notice('warning', 'Cannot Save As on a special buffer.')
        return
    end

    save_prompt_buffer = bufnr
    local prompt = force_prompt
        and 'Save file as (Enter to save, Esc to cancel): '
        or 'Save new file as (Enter to save, Esc to cancel): '
    local ok, err = pcall(vim.ui.input, {
        prompt = prompt,
        completion = 'file',
    }, function(path)
        save_prompt_buffer = nil
        if path and path ~= '' then
            if vim.uv.fs_stat(path) then
                _G.tuim_native_notice('warning', 'That path already exists. Choose another filename.')
                ide_restore_cursor(saved_cursor)
            else
                write(path, force_prompt)
            end
        else
            if _G.tuim_notify_buffers then _G.tuim_notify_buffers() end
            ide_restore_cursor(saved_cursor)
        end
    end)
    if not ok then
        save_prompt_buffer = nil
        _G.tuim_native_notice('error', 'Could not open filename prompt: ' .. tostring(err))
        ide_restore_cursor(saved_cursor)
    end
end

_G.tuim_save_file = function()
    tuim_save_buffer_impl(vim.api.nvim_get_current_buf(), {})
end

_G.tuim_save_file_as = function()
    tuim_save_buffer_impl(vim.api.nvim_get_current_buf(), { force_prompt = true })
end

do
    local project_builds = {
        rust = { ["Cargo.toml"] = "cargo build" },
        go = { ["go.mod"] = "go build ./..." },
        c = { ["CMakeLists.txt"] = "cmake -S . -B build && cmake --build build" },
        javascript = { ["package.json"] = "npm run build" },
    }
    project_builds.cpp = project_builds.c
    project_builds.javascriptreact = project_builds.javascript
    project_builds.typescript = project_builds.javascript
    project_builds.typescriptreact = project_builds.javascript

    local makefiles = { "GNUmakefile", "makefile", "Makefile" }
    local file_builds = {
        c = "cc ",
        cpp = "c++ ",
        go = "go build ",
        java = "javac ",
        python = "python3 -m py_compile ",
        rust = "rustc ",
        typescript = "tsc ",
    }
    local custom_commands = {}

    local function compile_defaults(win)
        local buf = vim.api.nvim_win_get_buf(win)
        local path = vim.bo[buf].buftype == "" and vim.api.nvim_buf_get_name(buf) or ""
        local ft = vim.bo[buf].filetype
        if ft == "" and path ~= "" then
            ft = vim.filetype.match({ filename = path, buf = buf }) or ""
        end
        local cwd = vim.fn.getcwd(vim.api.nvim_win_get_number(win))
        local start = path ~= "" and vim.fs.dirname(path) or cwd
        local builds = project_builds[ft] or {}
        local markers = vim.list_extend(vim.tbl_keys(builds), makefiles)
        local root = vim.fs.root(start, markers)
        if root then
            for marker, command in pairs(builds) do
                if vim.fn.filereadable(vim.fs.joinpath(root, marker)) == 1 then
                    return root, command, root .. "\0" .. ft
                end
            end
            return root, "make", root .. "\0" .. ft
        end
        local command = file_builds[ft]
        if path ~= "" and command then
            return start, command .. vim.fn.shellescape(path), path .. "\0" .. ft
        end
        return cwd, "", cwd .. "\0" .. ft
    end

    local function open_compile_output(command, cwd)
        local width = math.max(1, math.floor(vim.o.columns * 0.85))
        local height = math.max(1, math.floor(vim.o.lines * 0.7))
        local buf = vim.api.nvim_create_buf(false, true)
        vim.bo[buf].bufhidden = "wipe"
        local win = vim.api.nvim_open_win(buf, true, {
            relative = "editor",
            row = math.max(0, math.floor((vim.o.lines - height - 2) / 2)),
            col = math.max(0, math.floor((vim.o.columns - width - 2) / 2)),
            width = width,
            height = height,
            style = "minimal",
            border = "rounded",
            title = " Compile output · Running ",
            footer = " q / Esc to close ",
        })
        vim.cmd.stopinsert()
        local function close()
            vim.api.nvim_win_close(win, true)
        end
        vim.keymap.set("n", "q", close, { buffer = buf })
        vim.keymap.set("n", "<Esc>", close, { buffer = buf })

        local ok, job = pcall(vim.fn.jobstart, command, {
            term = true,
            cwd = cwd,
            on_exit = function(_, code)
                if vim.api.nvim_win_is_valid(win) and vim.api.nvim_win_get_buf(win) == buf then
                    vim.api.nvim_win_set_config(win, { title = (" Compile output · Exit %d "):format(code) })
                end
            end,
        })
        if not ok or job <= 0 then
            local message = ok and "Could not start the shell." or tostring(job)
            vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "Compilation could not start.", message })
            vim.bo[buf].modifiable = false
            vim.api.nvim_win_set_config(win, { title = " Compile output · Failed to start " })
        end
    end

    vim.api.nvim_create_user_command("TuimCompile", function()
        local win = vim.api.nvim_get_current_win()
        if vim.api.nvim_win_get_config(win).relative ~= "" then
            for _, candidate in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
                if vim.api.nvim_win_get_config(candidate).relative == "" then
                    win = candidate
                    break
                end
            end
        end
        local cwd, default, key = compile_defaults(win)
        if vim.g.tuim_gui_compile then
            vim.rpcnotify(1, "tuim_compile_open", {
                cwd = cwd,
                command = custom_commands[key] or default,
                default_command = default,
                key = key,
            })
            return
        end
        vim.ui.input({ prompt = "Compile command: ", default = custom_commands[key] or default }, function(command)
            if not command or vim.trim(command) == "" then return end
            custom_commands[key] = command ~= default and command or nil
            open_compile_output(command, cwd)
        end)
    end, { desc = "Edit and run a build command in a floating window" })
end

_G.tuim_ide_action = function(action)
    local mode = vim.api.nvim_get_mode().mode
    local visual = mode:match('[vV\22]') ~= nil
    if action == 'save' then _G.tuim_save_file(); return
    elseif action == 'save_as' then _G.tuim_save_file_as(); return
    elseif action == 'undo' then pcall(vim.cmd, 'undo')
    elseif action == 'redo' then pcall(vim.cmd, 'redo')
    elseif action == 'select_all' or action == 'select_line' then
        -- `:normal! V` from an insert-mode mapping leaves Visual half-entered:
        -- later keys still resolve as Insert mode, so copy/cut never saw a selection.
        if mode:sub(1, 1) ~= 'i' then vim.cmd(action == 'select_all' and 'normal! ggVG' or 'normal! V'); return end
        local keys = action == 'select_all' and '<C-o>gg<C-o>VG' or '<C-o>V'
        -- 'ni' prepends: keys batched behind this one must not run before the selection exists.
        vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(keys, true, false, true), 'ni', false)
        return
    elseif action == 'copy' and visual then
        vim.cmd('normal! "' .. ide_clipboard_register() .. 'y')
    elseif action == 'cut' and visual then
        vim.cmd('normal! "' .. ide_clipboard_register() .. 'd')
    elseif action == 'paste' then
        -- Visual P replaces the selection without clobbering the clipboard.
        vim.cmd('normal! "' .. ide_clipboard_register() .. (visual and 'P' or 'p'))
    elseif action == 'find' then
        if vim.fn.exists(':Telescope') == 2 then vim.cmd('Telescope current_buffer_fuzzy_find')
        else
            cmdline_action_pending = true
            local prefix = (mode:sub(1, 1) == 'i' or visual) and '<Esc>' or ''
            vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(prefix .. '/', true, false, true), 'ni', false)
            return
        end
    elseif action == 'replace' then
        -- Exit Insert/Visual cleanly with <Esc> instead of <C-o> so running
        -- :%s/...<CR> never stalls in Neovim's <C-o> wait_return loop, which
        -- otherwise blocks dirty notifications and immediate Ctrl+S saves.
        cmdline_action_pending = true
        local prefix = (mode:sub(1, 1) == 'i' or visual) and '<Esc>' or ''
        vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(prefix .. ':%s/', true, false, true), 'ni', false)
        return
    elseif action == 'new' then vim.cmd('enew')
    elseif action == 'close' then _G.tuim_close_buffer(vim.api.nvim_get_current_buf()); return
    elseif action == 'next_buffer' then vim.cmd('bnext')
    elseif action == 'previous_buffer' then vim.cmd('bprevious')
    end
    ide_startinsert()
end

local function ide_action_map(modes, lhs, action)
    vim.keymap.set(modes, lhs, function() _G.tuim_ide_action(action) end,
        { silent = true, desc = 'IDE: ' .. action:gsub('_', ' ') })
end

-- Ensure Ctrl+Shift+S never inserts literal ^S (0x13) in any mode.
vim.keymap.set({ 'i', 'n', 'v', 's' }, '<C-S-s>', function() _G.tuim_save_file_as() end,
    { silent = true, desc = 'Save file as' })

_G.tuim_enable_ide_mode = function()
    vim.g.tuim_ide_mode = true
    for mode, mappings in pairs(ide_mappings) do
        for lhs, rhs in pairs(mappings) do vim.keymap.set(mode, lhs, rhs, { silent = true, desc = 'IDE mode' }) end
    end
    for _, spec in ipairs(ide_undo_break_keys) do
        vim.keymap.set('i', spec[1], function()
            if vim.bo.buftype == '' then return spec[2] end
            return spec[3]
        end, { expr = true, replace_keycodes = true, silent = true, desc = 'IDE mode undo break' })
    end
    if not package.loaded['blink.cmp'] then
        ide_cr_fallback_mapped = true
        vim.keymap.set('i', '<CR>', function()
            if vim.bo.buftype == '' then return '<CR><C-g>u' end
            return '<CR>'
        end, { expr = true, replace_keycodes = true, silent = true, desc = 'IDE mode newline undo break' })
    end
    for _, spec in ipairs({
        { '<C-s>', 'save' }, { '<D-s>', 'save' }, { '<C-S-s>', 'save_as' }, { '<D-S-s>', 'save_as' },
        { '<C-z>', 'undo' }, { '<D-z>', 'undo' }, { '<C-y>', 'redo' }, { '<C-S-z>', 'redo' }, { '<D-S-z>', 'redo' },
        { '<C-a>', 'select_all' }, { '<D-a>', 'select_all' }, { '<C-l>', 'select_line' },
        { '<C-c>', 'copy' }, { '<D-c>', 'copy' }, { '<C-x>', 'cut' }, { '<D-x>', 'cut' },
        { '<C-v>', 'paste' }, { '<D-v>', 'paste' }, { '<C-f>', 'find' }, { '<D-f>', 'find' },
        { '<C-h>', 'replace' }, { '<D-r>', 'replace' },
    }) do ide_action_map({ 'i', 'n', 'v', 's' }, spec[1], spec[2]) end
    ide_startinsert()
    local ide_group = vim.api.nvim_create_augroup("TuimIdeMode", { clear = true })
    vim.api.nvim_create_autocmd({ "BufEnter", "BufWinEnter", "WinEnter", "ModeChanged", "TermLeave" }, {
        group = ide_group,
        callback = function(event)
            if event.event ~= 'ModeChanged' or vim.api.nvim_get_mode().mode == 'n' then
                ide_startinsert()
            end
        end,
    })
    vim.api.nvim_create_autocmd("CmdlineLeave", {
        group = ide_group,
        callback = function()
            cmdline_action_pending = false
            ide_startinsert()
        end,
    })
    vim.api.nvim_create_autocmd("WinClosed", {
        group = ide_group,
        callback = function()
            ide_startinsert()
            vim.defer_fn(function() ide_startinsert() end, 25)
        end,
    })
    pcall(vim.rpcnotify, 1, "tuim_mode_sync", "ide")
end

_G.tuim_disable_ide_mode = function()
    vim.g.tuim_ide_mode = false
    cmdline_action_pending = false
    vim.cmd("stopinsert")
    for mode, mappings in pairs(ide_mappings) do
        for lhs in pairs(mappings) do pcall(vim.keymap.del, mode, lhs) end
    end
    for _, spec in ipairs(ide_undo_break_keys) do
        pcall(vim.keymap.del, 'i', spec[1])
    end
    if ide_cr_fallback_mapped then
        ide_cr_fallback_mapped = false
        pcall(vim.keymap.del, 'i', '<CR>')
    end
    for _, lhs in ipairs({ '<C-s>', '<D-s>', '<D-S-s>', '<C-z>', '<D-z>', '<C-y>', '<C-S-z>', '<D-S-z>',
        '<C-a>', '<D-a>', '<C-l>', '<C-c>', '<D-c>', '<C-x>', '<D-x>', '<C-v>', '<D-v>',
        '<C-f>', '<D-f>', '<C-h>', '<D-r>' }) do
        for _, mode in ipairs({ 'i', 'n', 'v', 's' }) do pcall(vim.keymap.del, mode, lhs) end
    end
    pcall(vim.api.nvim_del_augroup_by_name, "TuimIdeMode")
    pcall(vim.rpcnotify, 1, "tuim_mode_sync", vim.g.tuim_zen_mode and "zen" or "normal")
end

-- System reads only the desktop theme spec, never the user's Neovim init.
do
    local enabled, timer, signature, applying = false, nil, nil, false
    local theme_runtime
    local function system_highlight(buf)
        if not enabled or vim.bo[buf].buftype ~= '' then return end
        local lang = vim.treesitter.language.get_lang(vim.bo[buf].filetype)
        if not lang then return end
        local ok, loaded = pcall(vim.treesitter.language.add, lang)
        if not ok or not loaded then
            local data = vim.env.XDG_DATA_HOME or ((vim.env.HOME or vim.fn.expand('~')) .. '/.local/share')
            local site = data .. '/nvim/site'
            -- Reuse only installed parser/query assets, without loading nvim's config.
            ok, loaded = pcall(vim.treesitter.language.add, lang, { path = site .. '/parser/' .. lang .. '.so' })
            if not ok or not loaded then return end
            local query = site .. '/queries/' .. lang .. '/highlights.scm'
            if vim.fn.filereadable(query) == 1 then
                pcall(vim.treesitter.query.set, lang, 'highlights', table.concat(vim.fn.readfile(query), '\n'))
            end
        end
        pcall(vim.treesitter.start, buf, lang)
    end
    local function stop()
        enabled, signature = false, nil
        if timer then timer:stop(); timer:close(); timer = nil end
        _G.tuim_system_palette = nil
        _G.tuim_system_colorscheme = nil
        if theme_runtime then vim.opt.rtp:remove(theme_runtime); theme_runtime = nil end
    end
    local function desktop_theme(path)
        local file = io.open(path, 'r')
        if not file then return nil, '' end
        local content = file:read(65537) or ''
        file:close()
        if #content > 65536 then return nil, '' end
        -- Desktop theme files are declarative Lazy specs. An empty environment
        -- excludes config side effects; unsupported specs use the palette.
        local chunk = loadstring(content, '@' .. path)
        if not chunk then return nil, content end
        setfenv(chunk, {})
        local ok, specs = pcall(chunk)
        if not ok or type(specs) ~= 'table' then return nil, content end
        local name
        for _, spec in ipairs(specs) do
            if type(spec) == 'table' and spec[1] == 'LazyVim/LazyVim' and type(spec.opts) == 'table' then
                name = spec.opts.colorscheme
            end
        end
        if type(name) ~= 'string' or not name:match('^[%w_.-]+$') then return nil, content end
        local data = vim.env.XDG_DATA_HOME or ((vim.env.HOME or vim.fn.expand('~')) .. '/.local/share')
        for _, spec in ipairs(specs) do
            local repo = type(spec) == 'table' and spec[1] or spec
            if type(repo) == 'string' and repo ~= 'LazyVim/LazyVim' then
                local directory = repo:match('^[%w_.-]+/([%w_.-]+)$')
                if directory then
                    for _, root in ipairs({ vim.fn.stdpath('data'), data .. '/nvim' }) do
                        local runtime = root .. '/lazy/' .. directory
                        if vim.uv.fs_stat(runtime .. '/colors/' .. name .. '.lua') or vim.uv.fs_stat(runtime .. '/colors/' .. name .. '.vim') then
                            return { name = name, runtime = runtime }, content .. runtime
                        end
                    end
                end
            end
        end
        return nil, content
    end
    local function mix(a, b, amount)
        local out = '#'
        for i = 2, 6, 2 do
            local x, y = tonumber(a:sub(i, i + 1), 16), tonumber(b:sub(i, i + 1), 16)
            out = out .. string.format('%02x', math.floor(x + (y - x) * amount + 0.5))
        end
        return out
    end
    local function read_palette()
        local home = vim.env.HOME or vim.fn.expand('~')
        local state = vim.env.XDG_STATE_HOME or (home .. '/.local/state')
        local config = vim.env.XDG_CONFIG_HOME or (home .. '/.config')
        for _, root in ipairs({ state, config }) do
            local path = root .. '/omarchy/current/theme/colors.toml'
            local file = io.open(path, 'r')
            if file then
                local content = file:read(65537) or ''
                file:close()
                if #content <= 65536 then
                    local p = {}
                    for line in content:gmatch('[^\r\n]+') do
                        local key, value = line:match('^%s*([%w_]+)%s*=%s*["\'](#[%x]+)["\']')
                        if key and #value == 7 then p[key] = value end
                        local mode = line:match('^%s*mode%s*=%s*["\'](%a+)["\']')
                        if mode == 'dark' or mode == 'light' then p.mode = mode end
                    end
                    if p.background and p.foreground then
                        local theme, spec = desktop_theme(root .. '/omarchy/current/theme/neovim.lua')
                        p.desktop_theme = theme
                        return p, path .. '\n' .. content .. '\n' .. spec
                    end
                end
            end
        end
    end
    local function apply(p)
        applying = true
        local bg, fg = p.background, p.foreground
        local brightness = tonumber(bg:sub(2, 3), 16) * 0.299
            + tonumber(bg:sub(4, 5), 16) * 0.587 + tonumber(bg:sub(6, 7), 16) * 0.114
        vim.g.colors_name = nil
        vim.o.background = p.mode or (brightness > 128 and 'light' or 'dark')
        vim.cmd('highlight clear')
        if vim.g.syntax_on then vim.cmd('syntax reset') end
        local accent = p.accent or p.blue or p.color4 or fg
        local sidebar = p.dark_background or mix(bg, fg, 0.04)
        local raised = p.lighter_background or mix(bg, fg, 0.08)
        local muted = p.light_foreground or mix(bg, fg, 0.6)
        local border = p.muted or mix(bg, fg, 0.3)
        local red, green = p.red or p.color1 or accent, p.green or p.color2 or accent
        local yellow, blue = p.yellow or p.color3 or accent, p.blue or p.color4 or accent
        local magenta, cyan = p.magenta or p.color5 or accent, p.cyan or p.color6 or accent
        p.selection_background = p.selection_background or p.selection or mix(bg, accent, 0.35)
        p.selection_foreground = p.selection_foreground or fg
        p.terminal_colors = { p.color0 or bg, red, green, yellow, blue, magenta, cyan,
            p.color7 or fg, p.color8 or muted, p.bright_red or p.color9 or red,
            p.bright_green or p.color10 or green, p.bright_yellow or p.color11 or yellow,
            p.bright_blue or p.color12 or blue, p.bright_magenta or p.color13 or magenta,
            p.bright_cyan or p.color14 or cyan, p.bright_foreground or p.color15 or fg }
        local highlights = {
            Normal = { fg = fg, bg = bg }, NormalNC = { fg = fg, bg = bg },
            NormalFloat = { fg = fg, bg = sidebar }, FloatBorder = { fg = border, bg = sidebar },
            NeoTreeNormal = { fg = fg, bg = sidebar }, NeoTreeNormalNC = { fg = fg, bg = sidebar },
            NvimTreeNormal = { fg = fg, bg = sidebar },
            Comment = { fg = muted, italic = true }, Identifier = { fg = fg },
            Constant = { fg = p.orange or yellow }, String = { fg = green },
            Character = { fg = green }, Number = { fg = p.orange or yellow },
            Boolean = { fg = p.orange or yellow }, Float = { fg = p.orange or yellow },
            Function = { fg = blue }, Statement = { fg = magenta }, Keyword = { fg = magenta },
            Operator = { fg = accent }, PreProc = { fg = cyan }, Type = { fg = yellow },
            Special = { fg = cyan }, Delimiter = { fg = fg }, Underlined = { fg = blue, underline = true },
            Error = { fg = red }, ErrorMsg = { fg = red }, WarningMsg = { fg = yellow },
            Todo = { fg = bg, bg = accent, bold = true },
            DiagnosticError = { fg = red }, DiagnosticWarn = { fg = yellow },
            DiagnosticInfo = { fg = blue }, DiagnosticHint = { fg = cyan },
            Cursor = { fg = bg, bg = p.cursor or fg },
            CursorLine = { bg = raised }, CursorColumn = { bg = raised },
            LineNr = { fg = muted, bg = bg }, CursorLineNr = { fg = accent, bold = true },
            SignColumn = { bg = bg }, FoldColumn = { fg = muted, bg = bg },
            Folded = { fg = muted, bg = raised }, EndOfBuffer = { fg = border },
            WinSeparator = { fg = border, bg = bg }, VertSplit = { fg = border, bg = bg },
            Visual = { fg = p.selection_foreground, bg = p.selection_background },
            Search = { fg = bg, bg = yellow }, IncSearch = { fg = bg, bg = accent },
            Pmenu = { fg = fg, bg = sidebar }, PmenuSel = { fg = bg, bg = accent },
            PmenuSbar = { bg = raised }, PmenuThumb = { bg = muted },
            StatusLine = { fg = bg, bg = accent }, StatusLineNC = { fg = muted, bg = sidebar },
            TabLine = { fg = muted, bg = raised }, TabLineSel = { fg = accent, bg = bg, bold = true },
            TabLineFill = { bg = sidebar }, TuimAccent = { fg = accent },
            DiffAdd = { bg = mix(bg, green, 0.2) }, DiffDelete = { fg = red, bg = mix(bg, red, 0.2) },
            DiffChange = { bg = mix(bg, blue, 0.2) }, DiffText = { bg = mix(bg, blue, 0.35) },
        }
        for name, hl in pairs(highlights) do vim.api.nvim_set_hl(0, name, hl) end
        for name, target in pairs({ ['@variable'] = 'Identifier', ['@function'] = 'Function',
            ['@keyword'] = 'Keyword', ['@string'] = 'String', ['@type'] = 'Type',
            ['@comment'] = 'Comment', ['@constant'] = 'Constant', ['@number'] = 'Number',
            ['@boolean'] = 'Boolean', ['@operator'] = 'Operator' }) do
            vim.api.nvim_set_hl(0, name, { link = target })
        end
        _G.tuim_system_colorscheme = nil
        if theme_runtime then vim.opt.rtp:remove(theme_runtime); theme_runtime = nil end
        if p.desktop_theme then
            theme_runtime = p.desktop_theme.runtime
            vim.opt.rtp:prepend(theme_runtime)
            _G.tuim_system_colorscheme = p.desktop_theme.name
            if not pcall(vim.cmd.colorscheme, p.desktop_theme.name) then
                p.desktop_theme = nil
                return apply(p)
            end
        end
        _G.tuim_system_palette = p
        vim.g.colors_name = 'system'
        vim.api.nvim_exec_autocmds('ColorScheme', { pattern = 'system', modeline = false })
        applying = false
        for _, buf in ipairs(vim.api.nvim_list_bufs()) do
            if vim.api.nvim_buf_is_loaded(buf) then system_highlight(buf) end
        end
    end
    local function refresh()
        if not enabled then return end
        local p, content = read_palette()
        -- Omarchy replaces the theme directory during a switch. Retain the
        -- last valid palette through that gap or an incomplete write.
        if p and content ~= signature then
            apply(p)
            signature = content
        end
    end
    local function dark_modern()
        vim.o.background = 'dark'
        local ok, vscode = pcall(require, 'vscode')
        if ok then
            vscode.setup({
                style = 'dark',
                color_overrides = {
                    vscBack = '#1F1F1F', vscFront = '#CCCCCC',
                    vscLeftDark = '#181818', vscPopupBack = '#202020',
                    vscLineNumber = '#6E7681',
                },
                group_overrides = {
                    TuimAccent = { fg = '#0078D4' },
                    WinSeparator = { fg = '#2B2B2B' },
                },
            })
            vscode.load('dark')
            vim.api.nvim_exec_autocmds('ColorScheme', { pattern = 'vscode', modeline = false })
        else
            vim.cmd('highlight clear')
            vim.g.colors_name = 'vscode'
            local groups = {
                Normal = { fg = '#CCCCCC', bg = '#1F1F1F' },
                NormalNC = { fg = '#CCCCCC', bg = '#181818' },
                LineNr = { fg = '#6E7681' }, CursorLineNr = { fg = '#CCCCCC' },
                SignColumn = { bg = '#1F1F1F' }, EndOfBuffer = { fg = '#1F1F1F' },
                WinSeparator = { fg = '#2B2B2B' }, TuimAccent = { fg = '#0078D4' },
                Visual = { bg = '#264F78' }, Search = { bg = '#9E6A03' },
                Pmenu = { fg = '#CCCCCC', bg = '#202020' }, PmenuSel = { bg = '#04395E' },
                Comment = { fg = '#6A9955' }, String = { fg = '#CE9178' },
                Character = { fg = '#CE9178' }, Number = { fg = '#B5CEA8' },
                Boolean = { fg = '#569CD6' }, Float = { fg = '#B5CEA8' },
                Identifier = { fg = '#9CDCFE' }, Function = { fg = '#DCDCAA' },
                Statement = { fg = '#C586C0' }, Keyword = { fg = '#569CD6' },
                PreProc = { fg = '#C586C0' }, Type = { fg = '#4EC9B0' },
                Special = { fg = '#D7BA7D' }, Constant = { fg = '#4FC1FF' },
                Operator = { fg = '#D4D4D4' }, Delimiter = { fg = '#CCCCCC' },
                DiagnosticError = { fg = '#F85149' }, DiagnosticWarn = { fg = '#CCA700' },
                DiagnosticInfo = { fg = '#3794FF' }, DiagnosticHint = { fg = '#4EC9B0' },
            }
            for name, attrs in pairs(groups) do vim.api.nvim_set_hl(0, name, attrs) end
            for capture, group in pairs({
                comment = 'Comment', string = 'String', number = 'Number', boolean = 'Boolean',
                variable = 'Identifier', ['variable.member'] = 'Identifier',
                ['function'] = 'Function', ['function.call'] = 'Function',
                ['function.method'] = 'Function', ['function.builtin'] = 'Function',
                keyword = 'Keyword', ['keyword.return'] = 'Statement',
                ['keyword.conditional'] = 'Statement', ['keyword.repeat'] = 'Statement',
                type = 'Type', ['type.builtin'] = 'Type', constant = 'Constant',
                operator = 'Operator', ['punctuation.bracket'] = 'Delimiter',
            }) do vim.api.nvim_set_hl(0, '@' .. capture, { link = group }) end
            vim.api.nvim_exec_autocmds('ColorScheme', { pattern = 'vscode', modeline = false })
        end
    end
    _G.tuim_apply_theme = function(name)
        if name ~= 'system' then
            stop()
            pcall(vim.api.nvim_set_hl, 0, 'TuimAccent', {})
            if name == 'vscode' then return dark_modern() end
            local ok, err = pcall(vim.cmd.colorscheme, name)
            if ok then return end
            if type(name) == 'string' and name:match('^[%w_.-]+$') then
                local local_colors = user_config_dir .. '/colors/' .. name
                if vim.uv.fs_stat(local_colors .. '.lua') or vim.uv.fs_stat(local_colors .. '.vim') then
                    if not vim.tbl_contains(vim.opt.rtp:get(), user_config_dir) then
                        vim.opt.rtp:append(user_config_dir)
                    end
                    return vim.cmd.colorscheme(name)
                end
            end
            error(err)
        end
        if not enabled then
            enabled = true
            refresh()
            if not signature then
                applying = true
                vim.cmd.colorscheme('default')
                vim.g.colors_name = 'system'
                vim.api.nvim_exec_autocmds('ColorScheme', { pattern = 'system', modeline = false })
                applying = false
                _G.tuim_native_notice('warning', 'System palette unavailable; using default colors until a desktop palette is available.')
            end
            timer = vim.uv.new_timer()
            timer:start(1500, 1500, vim.schedule_wrap(refresh))
        else
            refresh()
        end
    end
    local group = vim.api.nvim_create_augroup('TuimSystemTheme', { clear = true })
    vim.api.nvim_create_autocmd('FileType', { group = group, callback = function(event) system_highlight(event.buf) end })
    vim.api.nvim_create_autocmd({ 'FocusGained', 'VimResume' }, { group = group, callback = refresh })
    vim.api.nvim_create_autocmd('ColorScheme', { group = group, callback = function()
        if not applying and vim.g.colors_name ~= 'system' then stop() end
    end })
    vim.api.nvim_create_autocmd('VimLeavePre', { group = group, callback = stop })
end

_G.tuim_save_settings = function()
    local state = {}
    local path = vim.g.tuim_settings_path or (vim.fn.stdpath("data") .. "/settings.json")
    local f = io.open(path, "r")
    if f then
        local content = f:read("*a")
        f:close()
        local ok, s = pcall(vim.fn.json_decode, content)
        if ok and type(s) == "table" then
            state = s
        end
    end
    local mode = "normal"
    if vim.g.tuim_zen_mode then
        mode = "zen"
    elseif vim.g.tuim_ide_mode then
        mode = "ide"
    end
    state.mode = mode
    state.zen = (mode == "zen")
    state.ide = (mode == "ide")
    state.clip = vim.o.clipboard:match("unnamedplus") ~= nil
    state.theme = vim.g.colors_name or "vscode"
    state.autocomplete = vim.g.tuim_autocomplete_enabled ~= false
    state.autoindent = vim.o.autoindent
    state.nerd_fonts = vim.g.tuim_nerd_fonts ~= false
    state.zen_handoff = vim.g.tuim_zen_handoff == true
    local f_w = io.open(path, "w")
    if f_w then f_w:write(vim.fn.json_encode(state)); f_w:close() end
end

-- 'number' and 'relativenumber' are window-local: a dashboard's :setlocal
-- nonumber otherwise leaks into the first edited file and any inherited splits.
-- Keep the saved preference separate from whichever (possibly special) window
-- is current. Only entry/classification events touch options, never scrolling.
local tuim_numbers = "relative"
local function apply_numbers_to_window(win)
    if not vim.api.nvim_win_is_valid(win) then return end
    local buf = vim.api.nvim_win_get_buf(win)
    local bo = vim.bo[buf]
    local normal = (bo.buftype == "" or bo.buftype == "acwrite")
        and not ide_excluded_filetypes[bo.filetype]
        and vim.api.nvim_win_get_config(win).relative == ""
    local number = normal and tuim_numbers ~= "none"
    local relative = normal and tuim_numbers == "relative"
    -- Avoid unnecessary redraws/OptionSet work on repeated entry events.
    if vim.wo[win].number ~= number then vim.wo[win].number = number end
    if vim.wo[win].relativenumber ~= relative then vim.wo[win].relativenumber = relative end
end

_G.tuim_apply_numbers = function(value)
    tuim_numbers = (value == "absolute" or value == "none") and value or "relative"
    local number, relative = tuim_numbers ~= "none", tuim_numbers == "relative"
    -- Set defaults without enabling numbers in the current dashboard/picker.
    if vim.go.number ~= number then vim.go.number = number end
    if vim.go.relativenumber ~= relative then vim.go.relativenumber = relative end
    for _, win in ipairs(vim.api.nvim_list_wins()) do
        apply_numbers_to_window(win)
    end
end

local numbers_group = vim.api.nvim_create_augroup("TuimLineNumbers", { clear = true })
vim.api.nvim_create_autocmd({ "BufEnter", "BufWinEnter", "WinEnter", "FileType", "TermOpen" }, {
    group = numbers_group,
    callback = function(args)
        if args.event == "FileType" or args.event == "TermOpen" then
            for _, win in ipairs(vim.fn.win_findbuf(args.buf)) do apply_numbers_to_window(win) end
        else
            apply_numbers_to_window(vim.api.nvim_get_current_win())
        end
    end,
})

-- Some scratch/plugin buffers acquire their special type after BufEnter and
-- have no FileType event. Classification changes are rare, unlike cursor moves.
vim.api.nvim_create_autocmd("OptionSet", {
    group = numbers_group,
    pattern = "buftype",
    callback = function()
        -- OptionSet is not buffer-specific (args.buf is 0).
        for _, win in ipairs(vim.fn.win_findbuf(vim.api.nvim_get_current_buf())) do
            apply_numbers_to_window(win)
        end
    end,
})

local tuim_colorcolumn = ""
local function apply_colorcolumn_to_window(win)
    if not vim.api.nvim_win_is_valid(win) then return end
    local buf = vim.api.nvim_win_get_buf(win)
    local buftype = vim.bo[buf].buftype
    local filetype = vim.bo[buf].filetype
    local excluded = buftype ~= "" or filetype == "dashboard" or filetype == "alpha"
        or filetype == "help" or filetype == "tuim-settings"
    vim.wo[win].colorcolumn = excluded and "" or tuim_colorcolumn
end

_G.tuim_apply_colorcolumn = function(value)
    local allowed = { [""] = true, ["80"] = true, ["100"] = true,
        ["120"] = true, ["80,120"] = true }
    tuim_colorcolumn = allowed[value] and value or ""
    for _, win in ipairs(vim.api.nvim_list_wins()) do
        apply_colorcolumn_to_window(win)
    end
end

local colorcolumn_group = vim.api.nvim_create_augroup("TuimColorColumn", { clear = true })
vim.api.nvim_create_autocmd({ "BufEnter", "BufWinEnter", "FileType" }, {
    group = colorcolumn_group,
    callback = function(args)
        for _, win in ipairs(vim.fn.win_findbuf(args.buf)) do
            apply_colorcolumn_to_window(win)
        end
    end,
})

_G.tuim_load_settings = function()
    local f = io.open(vim.g.tuim_settings_path or (vim.fn.stdpath("data") .. "/settings.json"), "r")
    if not f then
        _G.tuim_apply_numbers("relative")
        -- Match Rust SettingsConfig::default() ("ide") when no settings.json exists yet.
        vim.g.tuim_zen_mode = false
        vim.g.tuim_ide_mode = true
        _G.tuim_enable_ide_mode()
        vim.schedule(ide_startinsert)
        return
    end
    local content = f:read("*a")
    f:close()
    local ok, state = pcall(vim.fn.json_decode, content)
    if not ok or type(state) ~= "table" then
        _G.tuim_apply_numbers("relative")
        vim.g.tuim_zen_mode = false
        vim.g.tuim_ide_mode = true
        _G.tuim_enable_ide_mode()
        vim.schedule(ide_startinsert)
        return
    end

    local mode = "ide"
    if state.mode ~= nil then
        mode = state.mode
    elseif state.zen then
        mode = "zen"
    elseif state.ide ~= nil then
        mode = state.ide and "ide" or "normal"
    end

    if mode == "zen" then
        vim.g.tuim_zen_mode = true
        vim.g.tuim_ide_mode = false
        _G.tuim_disable_ide_mode()
    elseif mode == "ide" then
        vim.g.tuim_zen_mode = false
        vim.g.tuim_ide_mode = true
        _G.tuim_enable_ide_mode()
        vim.schedule(ide_startinsert)
    else
        vim.g.tuim_zen_mode = false
        vim.g.tuim_ide_mode = false
        _G.tuim_disable_ide_mode()
    end

    local clip = state.clipboard
    if clip == nil then clip = state.clip end
    if clip ~= nil then
        vim.o.clipboard = clip and "unnamedplus" or ""
    end
    if state.autocomplete ~= nil then
        vim.g.tuim_autocomplete_enabled = state.autocomplete
    else
        vim.g.tuim_autocomplete_enabled = true
    end
    local term = os.getenv("TERM") or ""
    if state.nerd_fonts ~= nil then
        vim.g.tuim_nerd_fonts = state.nerd_fonts
    else
        vim.g.tuim_nerd_fonts = true
    end
    if term == "linux" then
        vim.g.tuim_nerd_fonts = false
    end
    if state.autoindent ~= nil then
        vim.o.autoindent = state.autoindent
    else
        vim.o.autoindent = true
    end
    _G.tuim_apply_numbers(state.numbers)
    if type(state.tabstop) == "number" and state.tabstop >= 2 and state.tabstop <= 8 then
        vim.o.tabstop = state.tabstop
        vim.o.shiftwidth = state.tabstop
    end
    if state.zen_handoff ~= nil then
        vim.g.tuim_zen_handoff = state.zen_handoff
    else
        vim.g.tuim_zen_handoff = false
    end
    if _G.tuim_apply_colorcolumn then
        _G.tuim_apply_colorcolumn(state.colorcolumn or "")
    end
    if state.theme then
        local theme_ok, err = pcall(_G.tuim_apply_theme, state.theme)
        if not theme_ok then
            vim.schedule(function()
                local retry_ok, retry_err = pcall(_G.tuim_apply_theme, state.theme)
                if not retry_ok then
                    pcall(_G.tuim_apply_theme, "vscode")
                    _G.tuim_native_notice(
                        "warning",
                        "Saved theme '" .. tostring(state.theme) .. "' unavailable ("
                            .. tostring(retry_err or err):gsub("^Vim:", "") .. "); using default theme."
                    )
                end
            end)
        end
    end
end

-- Count actual normal-file change batches, not renders, cursor movement or duplicate save notices.
if vim.g.tuim_settings_path then
    local activity_group = vim.api.nvim_create_augroup("GtuimProfileActivity", { clear = true })
    local activity_ticks = {}
    vim.api.nvim_create_autocmd({ "BufReadPost", "BufNewFile" }, {
        group = activity_group,
        callback = function(event)
            activity_ticks[event.buf] = vim.api.nvim_buf_get_changedtick(event.buf)
        end,
    })
    vim.api.nvim_create_autocmd("BufWipeout", {
        group = activity_group,
        callback = function(event) activity_ticks[event.buf] = nil end,
    })
    vim.api.nvim_create_autocmd({ "TextChanged", "TextChangedI", "BufWritePost" }, {
        group = activity_group,
        callback = function(event)
            local buf = event.buf
            if vim.bo[buf].buftype ~= "" or not vim.bo[buf].modifiable then return end
            if vim.api.nvim_buf_get_name(buf) == "" then return end
            local saved = event.event == "BufWritePost"
            if not saved then
                local tick = vim.api.nvim_buf_get_changedtick(buf)
                if activity_ticks[buf] == tick then return end
                activity_ticks[buf] = tick
                -- Loading/reloading a file also advances changedtick, but is not coding activity.
                if not vim.bo[buf].modified then return end
            end
            pcall(vim.rpcnotify, 1, "gtuim_code_activity", vim.bo[buf].filetype, saved)
        end,
    })
end

local M = {}
local themes = { "system", "vscode", "matteblack", "tokyonight", "tokyonight-storm", "catppuccin", "gruvbox", "nord", "cyberdream", "rose-pine", "kanagawa", "nightfox" }
function M.open()
    if vim.g.tuim_zen_mode == nil then vim.g.tuim_zen_mode = false end
    if vim.g.tuim_ide_mode == nil then vim.g.tuim_ide_mode = false end
    if vim.g.tuim_autocomplete_enabled == nil then vim.g.tuim_autocomplete_enabled = true end
    if vim.g.tuim_zen_handoff == nil then vim.g.tuim_zen_handoff = false end
    local nerd_fonts = vim.g.tuim_nerd_fonts ~= false
    local function get_toggle(is_on)
        if nerd_fonts then
            return is_on and " " or " "
        else
            return is_on and "[x] " or "[ ] "
        end
    end
    local current_theme = vim.g.colors_name or "vscode"
    
    local is_zen = vim.g.tuim_zen_mode == true
    local is_ide = vim.g.tuim_ide_mode == true
    local is_normal = not is_zen and not is_ide

    local width = 45
    local lines = { 
        string.rep(" ", width - 4) .. (nerd_fonts and "󰅖 " or "x "),
        "  General Settings",
        "  " .. get_toggle(is_zen) .. " Zen Mode                      [z]", 
        "  " .. get_toggle(is_ide) .. " IDE Mode                      [i]", 
        "  " .. get_toggle(is_normal) .. " Normal Mode                   [o]",
        "  " .. get_toggle(vim.g.tuim_zen_handoff) .. " Zen Handoff (Native Nvim)     [h]",
        "  " .. get_toggle(vim.o.clipboard:match("unnamedplus")) .. " System Clipboard              [c]",
        "  " .. get_toggle(vim.g.tuim_autocomplete_enabled) .. " Autocomplete                 [a]",
        "  " .. get_toggle(vim.o.autoindent) .. " Autoindent                   [n]",
        "", 
        "  Themes",
    }
    for _, t in ipairs(themes) do 
        table.insert(lines, "  " .. get_toggle(t == current_theme) .. " " .. t .. string.rep(" ", 30 - #t) .. "[t]") 
    end

    local height = #lines + 2
    local buf = vim.api.nvim_create_buf(false, true)
    local win = vim.api.nvim_open_win(buf, true, {
        relative = 'editor', width = width, height = height,
        row = math.floor((vim.o.lines - height) / 2), col = math.floor((vim.o.columns - width) / 2),
        style = 'minimal', border = 'rounded', title = ' Tuim Settings ', title_pos = 'center',
    })
    
    vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
    vim.bo[buf].modifiable = false
    vim.cmd("stopinsert")
    
    local function select_mode(mode)
        if mode == "zen" then
            vim.g.tuim_zen_mode = true
            vim.g.tuim_ide_mode = false
            _G.tuim_disable_ide_mode()
            vim.rpcnotify(1, "tuim_toggle_zen")
        elseif mode == "ide" then
            vim.g.tuim_zen_mode = false
            vim.g.tuim_ide_mode = true
            _G.tuim_enable_ide_mode()
            vim.rpcnotify(1, "tuim_toggle_ide")
        elseif mode == "normal" then
            vim.g.tuim_zen_mode = false
            vim.g.tuim_ide_mode = false
            _G.tuim_disable_ide_mode()
            vim.rpcnotify(1, "tuim_settings_changed")
        end
        if _G.tuim_save_settings then _G.tuim_save_settings() end
        pcall(vim.api.nvim_win_close, win, true)
        if mode ~= "zen" then
            require('tuim_settings').open()
        end
    end
    
    local function toggle_clipboard()
        if vim.o.clipboard:match("unnamedplus") then
            vim.o.clipboard = ""
            print("System Clipboard: OFF")
        else
            vim.o.clipboard = "unnamedplus"
            print("System Clipboard: ON")
        end
        if _G.tuim_save_settings then _G.tuim_save_settings() end
        pcall(vim.api.nvim_win_close, win, true)
        require('tuim_settings').open()
    end

    local function toggle_autocomplete()
        vim.g.tuim_autocomplete_enabled = not vim.g.tuim_autocomplete_enabled
        if _G.tuim_save_settings then _G.tuim_save_settings() end
        vim.rpcnotify(1, "tuim_settings_changed")
        pcall(vim.api.nvim_win_close, win, true)
        require('tuim_settings').open()
    end

    local function toggle_autoindent()
        vim.o.autoindent = not vim.o.autoindent
        if _G.tuim_save_settings then _G.tuim_save_settings() end
        vim.rpcnotify(1, "tuim_settings_changed")
        pcall(vim.api.nvim_win_close, win, true)
        require('tuim_settings').open()
    end
    
    local function toggle_zen_handoff()
        vim.g.tuim_zen_handoff = not vim.g.tuim_zen_handoff
        if _G.tuim_save_settings then _G.tuim_save_settings() end
        vim.rpcnotify(1, "tuim_settings_changed")
        pcall(vim.api.nvim_win_close, win, true)
        require('tuim_settings').open()
    end
    
    local function set_theme()
        local theme = vim.api.nvim_get_current_line():match("([%w%-]+)%s+%[t%]")
        if theme then 
            _G.tuim_apply_theme(theme)
            if _G.tuim_save_settings then _G.tuim_save_settings() end
            pcall(vim.api.nvim_win_close, win, true)
            require('tuim_settings').open()
        end
    end

    local function handle_click()
        local line = vim.api.nvim_get_current_line()
        if line:match("󰅖") or line:match("x ") or line:match("%[x%]") or vim.api.nvim_win_get_cursor(win)[1] == 1 then pcall(vim.api.nvim_win_close, win, true)
        elseif line:match("Zen Mode") then select_mode("zen")
        elseif line:match("IDE Mode") then select_mode("ide")
        elseif line:match("Normal Mode") then select_mode("normal")
        elseif line:match("Zen Handoff") then toggle_zen_handoff()
        elseif line:match("System Clipboard") then toggle_clipboard()
        elseif line:match("Autocomplete") then toggle_autocomplete()
        elseif line:match("Autoindent") then toggle_autoindent()
        else set_theme() end
    end

    local function do_click()
        vim.cmd("stopinsert")
        handle_click()
    end

    vim.keymap.set({'n', 'v', 'i'}, 'z', function() select_mode("zen") end, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, 'i', function() select_mode("ide") end, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, 'o', function() select_mode("normal") end, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, 'h', toggle_zen_handoff, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, 'c', toggle_clipboard, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, 'a', toggle_autocomplete, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, 'n', toggle_autoindent, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, 't', set_theme, { buffer = buf, silent = true })
    
    vim.keymap.set({'n', 'v', 'i'}, '<LeftMouse>', function()
        vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes("<LeftMouse>", true, false, true), "ntx", false)
        vim.schedule(do_click)
    end, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, '<LeftRelease>', function()
        vim.schedule(do_click)
    end, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, '<2-LeftMouse>', function()
        vim.schedule(do_click)
    end, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, '<CR>', do_click, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, 'q', function() pcall(vim.api.nvim_win_close, win, true) end, { buffer = buf, silent = true })
    vim.keymap.set({'n', 'v', 'i'}, '<Esc>', function() pcall(vim.api.nvim_win_close, win, true) end, { buffer = buf, silent = true })
end

function M.sync_theme()
    local function get_color(group, attr)
        local ok, hl = pcall(vim.api.nvim_get_hl, 0, { name = group, link = false })
        if ok and hl[attr] then return string.format("#%06x", hl[attr]) end
        return nil
    end
    local function adjust_color(hex, amount)
        if type(hex) ~= "string" or #hex ~= 7 then return hex end
        local r = tonumber(hex:sub(2, 3), 16) or 0
        local g = tonumber(hex:sub(4, 5), 16) or 0
        local b = tonumber(hex:sub(6, 7), 16) or 0
        r = math.max(0, math.min(255, r + amount))
        g = math.max(0, math.min(255, g + amount))
        b = math.max(0, math.min(255, b + amount))
        return string.format("#%02x%02x%02x", r, g, b)
    end
    local function get_contrast(hex, level)
        if type(hex) ~= "string" or #hex ~= 7 then return hex end
        local r = tonumber(hex:sub(2, 3), 16) or 0
        local g = tonumber(hex:sub(4, 5), 16) or 0
        local b = tonumber(hex:sub(6, 7), 16) or 0
        local brightness = (r * 299 + g * 587 + b * 114) / 1000
        return adjust_color(hex, brightness > 128 and -level or level)
    end

    local bg_editor = get_color("Normal", "bg") or "#1e1e1e"
    local fg_primary = get_color("Normal", "fg") or "#d4d4d4"
    local fg_secondary = get_color("Comment", "fg") or "#858585"
    local border_color = get_color("WinSeparator", "fg") or get_color("VertSplit", "fg") or "#3c3c3c"
    
    local bg_sidebar = get_color("NeoTreeNormal", "bg") or get_color("NvimTreeNormal", "bg") or get_color("NormalNC", "bg")
    if not bg_sidebar or bg_sidebar == bg_editor then
        bg_sidebar = get_contrast(bg_editor, 8)
    end
    
    local bg_tab_inactive = get_color("TabLine", "bg") or border_color
    if not bg_tab_inactive or bg_tab_inactive == bg_editor or bg_tab_inactive == border_color then
        bg_tab_inactive = get_contrast(bg_editor, 15)
    end
    
    local function blend_hex(a, b, ratio)
        if type(a) ~= "string" or #a ~= 7 or type(b) ~= "string" or #b ~= 7 then return a end
        local ar, ag, ab = tonumber(a:sub(2, 3), 16) or 0, tonumber(a:sub(4, 5), 16) or 0, tonumber(a:sub(6, 7), 16) or 0
        local br, bg, bb = tonumber(b:sub(2, 3), 16) or 0, tonumber(b:sub(4, 5), 16) or 0, tonumber(b:sub(6, 7), 16) or 0
        local r = math.floor(ar + (br - ar) * ratio + 0.5)
        local g = math.floor(ag + (bg - ag) * ratio + 0.5)
        local bl = math.floor(ab + (bb - ab) * ratio + 0.5)
        return string.format("#%02x%02x%02x", math.max(0, math.min(255, r)), math.max(0, math.min(255, g)), math.max(0, math.min(255, bl)))
    end
    local function color_chroma(hex)
        if type(hex) ~= "string" or #hex ~= 7 then return 0 end
        local r = tonumber(hex:sub(2, 3), 16) or 0
        local g = tonumber(hex:sub(4, 5), 16) or 0
        local b = tonumber(hex:sub(6, 7), 16) or 0
        return math.max(r, g, b) - math.min(r, g, b)
    end
    local function pick_accent()
        local explicit = get_color("TuimAccent", "fg")
        if explicit then return explicit end
        local scheme = tostring(vim.g.colors_name or ""):lower()
        local order
        if scheme:find("catppuccin", 1, true) then
            order = { "Keyword", "Statement", "Function", "Special", "Type" }
        elseif scheme:find("gruvbox", 1, true) then
            order = { "Special", "Constant", "Function", "Statement", "Type" }
        elseif scheme:find("cyberdream", 1, true) then
            order = { "Special", "Type", "Function", "Statement", "Keyword" }
        else
            order = { "Function", "Statement", "Keyword", "Special", "Type", "Identifier" }
        end
        local fallback = nil
        for _, grp in ipairs(order) do
            local c = get_color(grp, "fg")
            if c and c ~= bg_editor and c ~= fg_primary then
                if not fallback then fallback = c end
                if color_chroma(c) >= 28 then return c end
            end
        end
        return fallback or "#007acc"
    end

    local bg_accent = pick_accent()

    -- Some colorschemes make Visual indistinguishable from Normal once Tuim
    -- normalizes editor backgrounds. Keep mouse and keyboard selections clear
    -- while tinting them to match the active theme's accent.
    local system = vim.g.colors_name == 'system' and _G.tuim_system_palette or nil
    local selection_bg = system and system.selection_background
        or (vim.g.colors_name == "vscode" and "#264f78")
        or (vim.g.colors_name == "matteblack" and "#2d1b0d")
        or blend_hex(bg_editor, bg_accent, 0.28)
        or get_contrast(bg_editor, 36)
        or "#264f78"
    local selection_fg = system and system.selection_foreground or nil
    if not _G.tuim_system_colorscheme then
        vim.api.nvim_set_hl(0, "Visual", { fg = selection_fg, bg = selection_bg, bold = true })
        vim.api.nvim_set_hl(0, "VisualNOS", { fg = selection_fg, bg = selection_bg, bold = true })
    end
    
    local fg_statusbar = "#ffffff"
    do
        local r = tonumber(bg_accent:sub(2, 3), 16) or 0
        local g = tonumber(bg_accent:sub(4, 5), 16) or 0
        local b = tonumber(bg_accent:sub(6, 7), 16) or 0
        local brightness = (r * 299 + g * 587 + b * 114) / 1000
        if brightness > 128 then
            fg_statusbar = "#1e1e1e"
        end
    end

    -- Set terminal colors for the terminal panel so bash prompt ~ > is legible
    local bg_terminal = bg_editor
    
    vim.g.terminal_color_0  = get_color("Normal", "bg") or "#1e1e1e"
    vim.g.terminal_color_1  = get_color("Error", "fg") or "#f2495a"
    vim.g.terminal_color_2  = get_color("String", "fg") or "#42be65"
    vim.g.terminal_color_3  = get_color("WarningMsg", "fg") or "#ffcc00"
    vim.g.terminal_color_4  = get_color("Function", "fg") or "#007acc"
    vim.g.terminal_color_5  = get_color("Statement", "fg") or "#c678dd"
    vim.g.terminal_color_6  = get_color("Special", "fg") or "#56b6c2"
    vim.g.terminal_color_7  = get_color("Normal", "fg") or "#d4d4d4"
    vim.g.terminal_color_8  = get_color("Comment", "fg") or "#858585"
    vim.g.terminal_color_9  = get_color("Error", "fg") or "#f2495a"
    vim.g.terminal_color_10 = get_color("String", "fg") or "#42be65"
    vim.g.terminal_color_11 = get_color("WarningMsg", "fg") or "#ffcc00"
    vim.g.terminal_color_12 = get_color("Function", "fg") or "#007acc"
    vim.g.terminal_color_13 = get_color("Statement", "fg") or "#c678dd"
    vim.g.terminal_color_14 = get_color("Special", "fg") or "#56b6c2"
    vim.g.terminal_color_15 = "#ffffff"

    if system then
        for i, color in ipairs(system.terminal_colors) do vim.g['terminal_color_' .. (i - 1)] = color end
    end

    for _, buf in ipairs(vim.api.nvim_list_bufs()) do
        if vim.bo[buf].buftype == "terminal" then
            for i = 0, 15 do
                vim.api.nvim_buf_set_var(buf, "terminal_color_" .. i, vim.g["terminal_color_" .. i])
            end
            -- Force redraw in terminal buffer by briefly switching to it (optional but ensures update)
        end
    end

    vim.api.nvim_set_hl(0, "NormalFloat", { fg = fg_primary, bg = bg_sidebar })
    vim.api.nvim_set_hl(0, "FloatBorder", { fg = border_color, bg = bg_sidebar })

    local success_color = get_color("DiagnosticOk", "fg") or get_color("DiffAdd", "fg") or vim.g.terminal_color_2 or "#73c991"
    local warning_color = get_color("DiagnosticWarn", "fg") or vim.g.terminal_color_3 or "#e2c08d"
    local error_color = get_color("DiagnosticError", "fg") or vim.g.terminal_color_1 or "#f44747"

    local terminal_colors = {}
    for i = 0, 15 do terminal_colors[i + 1] = vim.g['terminal_color_' .. i] end
    pcall(vim.rpcnotify, 1, "tuim_theme_changed", {
        bg_editor = bg_editor, bg_sidebar = bg_sidebar, bg_tab_active = bg_editor,
        bg_tab_inactive = bg_tab_inactive, bg_statusbar = bg_accent, fg_statusbar = fg_statusbar,
        bg_accent = bg_accent, accent = bg_accent,
        success = success_color, warning = warning_color, error = error_color,
        fg_primary = fg_primary, fg_secondary = fg_secondary, fg_accent = fg_primary, border_color = border_color,
        bg_terminal = bg_terminal,
        terminal_colors = terminal_colors,
    })
end

package.loaded["tuim_settings"] = M

vim.api.nvim_create_autocmd("ColorScheme", {
    callback = function()
        if vim.g.colors_name == "matteblack" then
            local highlights = {
                Normal = { fg = "#dcdcdc", bg = "#080808" },
                NormalNC = { fg = "#777777", bg = "#080808" },
                Comment = { fg = "#4a4a4a", italic = true },
                Constant = { fg = "#ffaa66" },
                String = { fg = "#b8b8b8" },
                Character = { fg = "#b8b8b8" },
                Number = { fg = "#ffb86c" },
                Boolean = { fg = "#ffb86c" },
                Float = { fg = "#ffb86c" },
                Identifier = { fg = "#dcdcdc" },
                Function = { fg = "#eeeeee" },
                Statement = { fg = "#ff7300", bold = true },
                Conditional = { fg = "#ff7300", bold = true },
                Repeat = { fg = "#ff7300", bold = true },
                Label = { fg = "#ff7300", bold = true },
                Operator = { fg = "#a0a0a0" },
                Keyword = { fg = "#ff7300", bold = true },
                Exception = { fg = "#ff3333", bold = true },
                PreProc = { fg = "#888888" },
                Type = { fg = "#ffffff", bold = true },
                StorageClass = { fg = "#ffffff", bold = true },
                Structure = { fg = "#ffffff", bold = true },
                Typedef = { fg = "#ffffff", bold = true },
                Special = { fg = "#ffaa66" },
                Underlined = { underline = true },
                Error = { fg = "#ff3333", bg = "NONE", bold = true },
                Todo = { fg = "#080808", bg = "#ff7300", bold = true },
                
                CursorLine = { bg = "#141414" },
                CursorColumn = { bg = "#141414" },
                LineNr = { fg = "#3a3a3a", bg = "#080808" },
                CursorLineNr = { fg = "#ff7300", bg = "#141414", bold = true },
                SignColumn = { bg = "#080808" },
                FoldColumn = { bg = "#080808" },
                Folded = { fg = "#666666", bg = "#141414" },
                WinSeparator = { fg = "#1a1a1a", bg = "#080808" },
                VertSplit = { fg = "#1a1a1a", bg = "#080808" },
                
                Visual = { bg = "#2d1b0d" },
                Search = { fg = "#080808", bg = "#ff7300" },
                IncSearch = { fg = "#080808", bg = "#ffaa66" },
                
                Pmenu = { fg = "#dcdcdc", bg = "#141414" },
                PmenuSel = { fg = "#080808", bg = "#ff7300" },
                PmenuSbar = { bg = "#1a1a1a" },
                PmenuThumb = { bg = "#333333" },
                
                StatusLine = { fg = "#080808", bg = "#ff7300" },
                StatusLineNC = { fg = "#777777", bg = "#141414" },
                TabLine = { fg = "#777777", bg = "#141414" },
                TabLineSel = { fg = "#ff7300", bg = "#080808", bold = true },
                TabLineFill = { bg = "#141414" },
                
                ["@comment"] = { fg = "#4a4a4a", italic = true },
                ["@function"] = { fg = "#eeeeee" },
                ["@keyword"] = { fg = "#ff7300", bold = true },
                ["@string"] = { fg = "#b8b8b8" },
                ["@type"] = { fg = "#ffffff", bold = true },
                ["@variable"] = { fg = "#dcdcdc" },

                NeoTreeNormal = { fg = "#777777", bg = "#0c0c0c" },
                NeoTreeNormalNC = { fg = "#777777", bg = "#0c0c0c" },
                NeoTreeWinSeparator = { fg = "#1a1a1a", bg = "#0c0c0c" },

                TuimAccent = { fg = "#ff7300" },
            }
            for group, opts in pairs(highlights) do
                vim.api.nvim_set_hl(0, group, opts)
            end
        end
        require("tuim_settings").sync_theme()
    end,
})
vim.schedule(function() pcall(function() require("tuim_settings").sync_theme() end) end)

-- Apply the built-in default even when there is no saved settings file.
_G.tuim_apply_theme('vscode')
if _G.tuim_load_settings then _G.tuim_load_settings() end

-- Global function to restart dashboard when tabs close
_G.tuim_alpha_start = function()
    if not vim.g.tuim_gui_onboarding and vim.fn.exists(':Alpha') == 2 then
        vim.cmd("Alpha")
        return
    end
    local buf = vim.api.nvim_create_buf(false, true)
    vim.bo[buf].buftype = 'nofile'
    vim.bo[buf].bufhidden = 'wipe'
    vim.bo[buf].swapfile = false
    vim.bo[buf].filetype = 'tuim_dashboard'
    local lines = {
        '', '  tuim / ' .. vim.fn.fnamemodify(vim.fn.getcwd(), ':t'), '',
        '  New file          Ctrl+N',
        '  Find file         Ctrl+P', '',
        '  Commands          F1',
        '  Quit              Ctrl+Q',
    }
    vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
    vim.bo[buf].modifiable = false
    vim.api.nvim_win_set_buf(0, buf)
    vim.wo.number = false
    vim.wo.relativenumber = false
    vim.wo.signcolumn = 'no'
    vim.wo.colorcolumn = ''
end

-- The GUI paints a native welcome surface over the inert dashboard grid.
-- Hide it for real buffers, splits, floating pickers and command-line entry.
_G.tuim_publish_welcome = function()
    if not vim.g.tuim_gui_onboarding then return end
    local ft = vim.bo.filetype
    local active = (ft == 'tuim_dashboard' or ft == 'alpha' or ft == 'dashboard')
        and #vim.api.nvim_list_wins() == 1
        and vim.api.nvim_win_get_config(0).relative == ''
        and vim.fn.mode():sub(1, 1) ~= 'c'
    local cwd = vim.fn.fnamemodify(vim.fn.getcwd(), ':p'):gsub('/+$', '')
    if cwd == '' then cwd = '/' end
    local prefix = cwd == '/' and '/' or cwd .. '/'
    local recent, seen = {}, {}
    local function add(path)
        path = vim.fn.fnamemodify(path, ':p')
        if #recent < 6 and path:sub(1, #prefix) == prefix
            and not seen[path] and vim.fn.filereadable(path) == 1 then
            seen[path] = true
            recent[#recent + 1] = path
        end
    end
    if active then
        for _, info in ipairs(vim.fn.getbufinfo({ buflisted = 1 })) do
            if info.name ~= '' then add(info.name) end
        end
        for _, path in ipairs(vim.v.oldfiles) do add(path) end
    end
    pcall(vim.rpcnotify, 1, 'tuim_welcome', active, cwd, recent)
end
vim.api.nvim_create_autocmd({ 'BufEnter', 'BufWinEnter', 'WinEnter', 'WinClosed', 'FileType', 'DirChanged', 'ModeChanged' }, {
    group = vim.api.nvim_create_augroup('TuimNativeWelcome', { clear = true }),
    callback = function() vim.schedule(_G.tuim_publish_welcome) end,
})

-- Force dashboard on initial empty load
if vim.fn.argc() == 0 and not vim.g.tuim_is_terminal then
    vim.schedule(function()
        _G.tuim_alpha_start()
    end)
end

-- Nmux42 Bindings
-- Plugin commands are absent in recovery sessions (TUIM_DISABLE_PLUGINS=1).
local function plugin_command(command)
    return function()
        if vim.fn.exists(':' .. command:match('^%S+')) == 2 then vim.cmd(command)
        else _G.tuim_native_notice("warning", command:match('^%S+') .. " is unavailable while plugins are disabled.") end
    end
end
vim.keymap.set("n", "<leader>e", function()
    pcall(vim.rpcnotify, 1, "tuim_toggle_explorer")
end, { desc = "Toggle Explorer" })
vim.keymap.set("v", "J", ":m '>+1<CR>gv=gv")
vim.keymap.set("v", "K", ":m '<-2<CR>gv=gv")
vim.keymap.set("n", "J", "mzJ`z")
vim.keymap.set("n", "<C-d>", "<C-d>zz")
vim.keymap.set("n", "<C-u>", "<C-u>zz")
vim.keymap.set("n", "n", "nzzzv")
vim.keymap.set("n", "N", "Nzzzv")
vim.keymap.set("x", "<leader>p", [["_dP]])
vim.keymap.set({ "n", "v" }, "<leader>d", [["_d]])
vim.keymap.set("i", "<C-c>", "<Esc>")
vim.keymap.set("n", "<C-j>", "<cmd>cnext<CR>zz")
vim.keymap.set("n", "<C-k>", "<cmd>cprev<CR>zz")
vim.keymap.set("n", "Q", "<nop>")
vim.keymap.set("n", "<leader>k", "<cmd>lnext<CR>zz")
vim.keymap.set("n", "<leader>j", "<cmd>lprev<CR>zz")
vim.keymap.set("n", "<leader>cc", "<cmd>!php-cs-fixer fix % --using-cache=no<cr>")
vim.keymap.set("n", "<leader>s", [[:s/\<<C-r><C-w>\>//gI<Left><Left><Left>]])
vim.keymap.set("n", "<leader>x", "<cmd>!chmod +x %<CR>", { silent = true })
vim.keymap.set('n', '<leader>y', '<Plug>OSCYankOperator')
vim.keymap.set('v', '<leader>y', '<Plug>OSCYankVisual')
vim.keymap.set("n", "<leader>u", plugin_command("UndotreeToggle"))
vim.keymap.set("n", "<leader>cl", ":cclose<CR>", { silent = true })
vim.keymap.set("n", "<leader>co", ":copen<CR>", { silent = true })
vim.keymap.set("n", "<leader>cn", ":cnext<CR>zz")
vim.keymap.set("n", "<leader>cp", ":cprev<CR>zz")
vim.keymap.set("n", "<leader>mm", "<cmd>make<CR>")
vim.keymap.set("n", "<leader><leader>", function() vim.cmd("so") end)

-- Shell panes are owned by Tuim, including in Zen mode.
vim.keymap.set("n", "<leader>ot", function()
    vim.rpcnotify(1, "tuim_toggle_terminal", "bottom")
end, { desc = "Toggle native bottom terminal" })
vim.keymap.set("n", "<leader>oT", function()
    vim.rpcnotify(1, "tuim_toggle_terminal", "vertical")
end, { desc = "Toggle native vertical terminal" })
vim.keymap.set("n", "<leader>db", _G.tuim_alpha_start, { desc = "Go back to dashboard menu" })

vim.keymap.set("n", "<leader>th", "<cmd>lua require('tuim_settings').open()<cr>")

pcall(dofile, vim.fn.expand("~/.config/nmux42/plugin/vim_bindings.lua"))

local telescope_timer = nil
local prev_mt_rect = vim.NIL
local prev_pr_rect = vim.NIL
local prev_widget_title = vim.NIL

local function rects_eq(r1, r2)
    if r1 == vim.NIL and r2 == vim.NIL then return true end
    if r1 == vim.NIL or r2 == vim.NIL then return false end
    if type(r1) ~= "table" or type(r2) ~= "table" then return r1 == r2 end
    return r1[1] == r2[1] and r1[2] == r2[2] and r1[3] == r2[3] and r1[4] == r2[4]
end

local function notify_telescope()
    pcall(function()
        local mt_top, mt_left, mt_bottom, mt_right = 9999, 9999, -1, -1
        local pr_top, pr_left, pr_bottom, pr_right = 9999, 9999, -1, -1
        local found_mt = false
        local found_pr = false
        local widget_title = nil

        local has_telescope, action_state = pcall(require, "telescope.actions.state")
        local picker = nil
        if has_telescope and action_state then
            for _, w in ipairs(vim.api.nvim_list_wins()) do
                if vim.api.nvim_win_is_valid(w) then
                    local buf = vim.api.nvim_win_get_buf(w)
                    if buf and vim.api.nvim_buf_is_valid(buf) then
                        local ok_ft, ft = pcall(function() return vim.bo[buf].filetype end)
                        if ok_ft and ft == "TelescopePrompt" then
                            local ok_picker, p = pcall(action_state.get_current_picker, buf)
                            if ok_picker and p then
                                picker = p
                                break
                            end
                        end
                    end
                end
            end
        end

        if picker then
            local prompt_win = picker.prompt_win
            local results_win = picker.results_win
            local preview_win = picker.preview_win or (picker.previewer and picker.previewer.state and picker.previewer.state.winid)

            for _, w in ipairs({ prompt_win, results_win }) do
                if w and vim.api.nvim_win_is_valid(w) then
                    local cfg = vim.api.nvim_win_get_config(w)
                    if cfg and cfg.relative ~= "" then
                        local r = math.floor(cfg.row or 0)
                        local c = math.floor(cfg.col or 0)
                        local h = math.floor(cfg.height or 0)
                        local w_w = math.floor(cfg.width or 0)
                        
                        if r < 0 then r = 0 end
                        if c < 0 then c = 0 end

                        found_mt = true
                        widget_title = picker.prompt_title or "Search"
                        if r < mt_top then mt_top = r end
                        if c < mt_left then mt_left = c end
                        if r + h > mt_bottom then mt_bottom = r + h end
                        if c + w_w > mt_right then mt_right = c + w_w end
                    end
                end
            end

            if preview_win and vim.api.nvim_win_is_valid(preview_win) then
                local cfg = vim.api.nvim_win_get_config(preview_win)
                if cfg and cfg.relative ~= "" then
                    local r = math.floor(cfg.row or 0)
                    local c = math.floor(cfg.col or 0)
                    local h = math.floor(cfg.height or 0)
                    local w_w = math.floor(cfg.width or 0)
                    
                    if r < 0 then r = 0 end
                    if c < 0 then c = 0 end

                    found_pr = true
                    if r < pr_top then pr_top = r end
                    if c < pr_left then pr_left = c end
                    if r + h > pr_bottom then pr_bottom = r + h end
                    if c + w_w > pr_right then pr_right = c + w_w end
                end
            end
        end

        local mt_rect = found_mt and { mt_top, mt_left, mt_right - mt_left, mt_bottom - mt_top } or vim.NIL
        local pr_rect = found_pr and { pr_top, pr_left, pr_right - pr_left, pr_bottom - pr_top } or vim.NIL

        if not found_mt then
            for _, w in ipairs(vim.api.nvim_list_wins()) do
                if vim.api.nvim_win_is_valid(w) then
                    local buf = vim.api.nvim_win_get_buf(w)
                    if buf and vim.api.nvim_buf_is_valid(buf) then
                        local ok_ft, ft = pcall(function() return vim.bo[buf].filetype end)
                        if ok_ft and ft == "vimbindings" and (not vim.g.tuim_zen_mode) then
                            local cfg = vim.api.nvim_win_get_config(w)
                            if cfg and cfg.relative ~= "" then
                                local r = math.floor(cfg.row or 0)
                                local c = math.floor(cfg.col or 0)
                                local h = math.floor(cfg.height or 0)
                                local w_w = math.floor(cfg.width or 0)
                                if r < 0 then r = 0 end
                                if c < 0 then c = 0 end
                                found_mt = true
                                widget_title = " Tuim Help Reference "
                                mt_rect = { r, c, w_w, h }
                            end
                            break
                        end
                    end
                end
            end
        end

        if found_mt or found_pr then
            if not telescope_timer then
                telescope_timer = vim.uv.new_timer()
                telescope_timer:start(50, 50, vim.schedule_wrap(notify_telescope))
            end
            if not rects_eq(mt_rect, prev_mt_rect) or not rects_eq(pr_rect, prev_pr_rect) or widget_title ~= prev_widget_title then
                vim.rpcnotify(1, "tuim_telescope_rect", mt_rect, pr_rect, widget_title, picker ~= nil)
                prev_mt_rect = mt_rect
                prev_pr_rect = pr_rect
                prev_widget_title = widget_title
            end
        else
            if telescope_timer then
                telescope_timer:stop()
                telescope_timer:close()
                telescope_timer = nil
            end
            if prev_mt_rect ~= vim.NIL or prev_pr_rect ~= vim.NIL then
                vim.rpcnotify(1, "tuim_telescope_rect", vim.NIL, vim.NIL, vim.NIL)
                prev_mt_rect = vim.NIL
                prev_pr_rect = vim.NIL
                prev_widget_title = vim.NIL
            end
        end
    end)
end

-- Reserve a clean 1-row winbar per split when multiple splits are open so
-- Gtuim's native GPUI split header aligns pixel-for-pixel without overlapping
-- buffer line 1. Non-GUI sessions fall back to a clickable Neovim winbar.
local gui_split_spacer = "%#Normal# "
local split_close_suffix = "%=%#PmenuSel#%@v:lua.TuimClickCloseSplit@"
    .. "%{winwidth(0) >= 16 ? ' × Close split ' : '×'}%T%*"
local default_split_winbar = "%< %f %m"

local function update_split_winbar(win, show)
    local local_bar = vim.wo[win].winbar
    if vim.g.tuim_gui_split_headers then
        local has_spacer = local_bar == gui_split_spacer
        if show and not has_spacer then
            if vim.w[win].tuim_previous_winbar == nil then
                vim.w[win].tuim_previous_winbar = local_bar
            end
            vim.wo[win].winbar = gui_split_spacer
        elseif not show and has_spacer then
            vim.wo[win].winbar = vim.w[win].tuim_previous_winbar or ""
            vim.w[win].tuim_previous_winbar = nil
        end
        return
    end

    local has_close = local_bar:sub(-#split_close_suffix) == split_close_suffix
    if show and not has_close then
        local bar = local_bar == "" and vim.o.winbar or local_bar
        if bar == "" then bar = default_split_winbar end
        -- A leading %! evaluates the entire option as an expression.
        if bar:sub(1, 2) == "%!" then bar = "%{%" .. bar:sub(3) .. "%}" end
        vim.w[win].tuim_previous_winbar = local_bar
        vim.wo[win].winbar = bar .. split_close_suffix
    elseif not show and has_close then
        vim.wo[win].winbar = vim.w[win].tuim_previous_winbar or ""
        vim.w[win].tuim_previous_winbar = nil
    end
end

local function notify_win_positions()
    local win_list = {}
    local floats = {}
    local cur_win = vim.api.nvim_get_current_win()
    local splits = {}
    for _, w in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
        local cfg = vim.api.nvim_win_get_config(w)
        if cfg.relative == "" and not cfg.external then
            table.insert(splits, w)
        elseif cfg.relative ~= "" and not cfg.hide then
            local ok_pos, pos = pcall(vim.api.nvim_win_get_position, w)
            local ok_w, fw = pcall(vim.api.nvim_win_get_width, w)
            local ok_h, fh = pcall(vim.api.nvim_win_get_height, w)
            if ok_pos and ok_w and ok_h then
                local has_border = cfg.border ~= nil and cfg.border ~= "" and cfg.border ~= "none"
                local pad = has_border and 2 or 0
                table.insert(floats, {
                    row = pos[1],
                    col = pos[2],
                    width = fw + pad,
                    height = fh + pad,
                })
            end
        end
    end
    local show_winbar = (#splits > 1)
        or (vim.g.tuim_always_split_headers == true and not vim.g.tuim_zen_mode)
    for _, w in ipairs(splits) do
        update_split_winbar(w, show_winbar)
        local pos = vim.api.nvim_win_get_position(w) -- [row, col]
        local width = vim.api.nvim_win_get_width(w)
        local height = vim.api.nvim_win_get_height(w)
        local buf = vim.api.nvim_win_get_buf(w)
        local buf_name = vim.api.nvim_buf_get_name(buf)
        local short_name = vim.fn.fnamemodify(buf_name, ":t")
        if short_name == "" then short_name = "[No Name]" end
        local rel_name = buf_name == "" and "" or vim.fn.fnamemodify(buf_name, ":.")
        table.insert(win_list, {
            id = w,
            bufnr = buf,
            row = pos[1],
            col = pos[2],
            width = width,
            height = height,
            active = (w == cur_win),
            name = short_name,
            relative_name = rel_name,
            modified = vim.bo[buf].modified,
            readonly = vim.bo[buf].readonly,
            buftype = vim.bo[buf].buftype,
            is_ai = vim.b[buf].tuim_ai == true,
        })
    end
    vim.rpcnotify(1, "tuim_win_positions", win_list, floats)
end
_G.tuim_notify_win_positions = notify_win_positions

vim.api.nvim_create_autocmd({
    "WinNew", "WinClosed", "WinEnter", "WinLeave", "TabEnter",
    "WinResized", "VimResized", "BufEnter", "BufWinEnter",
    "BufFilePost", "BufModifiedSet", "BufWritePost", "TermOpen",
}, {
    callback = function()
        vim.schedule(notify_telescope)
        vim.schedule(notify_win_positions)
    end
})
vim.schedule(notify_win_positions)

_G.tuim_wincmd = function(dir)
    local current_win = vim.api.nvim_get_current_win()
    vim.cmd("wincmd " .. dir)
    if vim.api.nvim_get_current_win() == current_win then
        vim.rpcnotify(1, "tuim_boundary_hit", dir)
    end
end

vim.keymap.set({'i', 'n', 'v', 't'}, '<M-h>', function() _G.tuim_wincmd('h') end, { silent = true })
vim.keymap.set({'i', 'n', 'v', 't'}, '<M-j>', function() _G.tuim_wincmd('j') end, { silent = true })
vim.keymap.set({'i', 'n', 'v', 't'}, '<M-k>', function() _G.tuim_wincmd('k') end, { silent = true })
vim.keymap.set({'i', 'n', 'v', 't'}, '<M-l>', function() _G.tuim_wincmd('l') end, { silent = true })

vim.keymap.set({'i', 'n', 'v', 't'}, '<M-v>', function() vim.cmd("vsplit") end, { silent = true })
vim.keymap.set({'i', 'n', 'v', 't'}, '<M-s>', function() vim.cmd("split") end, { silent = true })
vim.keymap.set({'i', 'n', 'v', 't'}, '<M-c>', function() vim.cmd("close") end, { silent = true })
vim.keymap.set({'i', 'n', 'v', 't'}, '<M-o>', function() vim.cmd("wincmd w") end, { silent = true })

vim.schedule(function() pcall(notify_win_positions) end)

-- Native auto-pairs implementation (yields cleanly if windwp/nvim-autopairs is installed and active)
local external_autopairs = (not plugins_disabled)
    and plugin_inventory["windwp/nvim-autopairs"] ~= nil
    and plugin_inventory["windwp/nvim-autopairs"].enabled
    and not plugin_inventory["windwp/nvim-autopairs"].removed
    and vim.uv.fs_stat(lazy_root .. "/" .. plugin_inventory["windwp/nvim-autopairs"].name) ~= nil

if not external_autopairs then
local autopairs = {
    ["("] = ")",
    ["["] = "]",
    ["{"] = "}",
    ['"'] = '"',
    ["'"] = "'",
    ['`'] = '`',
}

for open_char, close_char in pairs(autopairs) do
    vim.keymap.set('i', open_char, function()
        local line = vim.api.nvim_get_current_line()
        local col = vim.api.nvim_win_get_cursor(0)[2]
        local next_char = line:sub(col + 1, col + 1)
        if open_char == close_char and next_char == close_char then
            return "<Right>"
        else
            return open_char .. close_char .. "<Left>"
        end
    end, { expr = true, replace_keycodes = true })
end

local closers = { ')', ']', '}', '"', "'", '`' }
for _, close_char in ipairs(closers) do
    vim.keymap.set('i', close_char, function()
        local line = vim.api.nvim_get_current_line()
        local col = vim.api.nvim_win_get_cursor(0)[2]
        local next_char = line:sub(col + 1, col + 1)
        if next_char == close_char then
            return "<Right>"
        else
            return close_char
        end
    end, { expr = true, replace_keycodes = true })
end

vim.keymap.set('i', '<BS>', function()
    local line = vim.api.nvim_get_current_line()
    local col = vim.api.nvim_win_get_cursor(0)[2]
    local prev_char = line:sub(col, col)
    local next_char = line:sub(col + 1, col + 1)
    if autopairs[prev_char] == next_char then
        return "<BS><Del>"
    else
        return "<BS>"
    end
end, { expr = true, replace_keycodes = true })
end

-- --- TUIM HELP WIDGET --- --
local VIM_MOTIONS = {
    "  ╭──────────────────────────────────────────────────────────────╮",
    "  │               ⚡  VIM MOTIONS REFERENCE CARD                │",
    "  ╰──────────────────────────────────────────────────────────────╯",
    "",
    "  ── MODES ──────────────────────────────────────────────────────",
    "  i / I      Insert mode (before cursor / at line start)",
    "  a / A      Append mode (after cursor / at line end)",
    "  o / O      Open new line below / above and insert",
    "  v / V      Visual mode / Visual Line mode",
    "  <C-v>      Visual Block mode (column editing)",
    "  Esc        Return to Normal mode",
    "  R          Replace mode (overwrite text)",
    "",
    "  ── NAVIGATION ─────────────────────────────────────────────────",
    "  h j k l    Left / Down / Up / Right",
    "  w / W      Jump to next word start (word / WORD)",
    "  e / E      Jump to next word end   (word / WORD)",
    "  b / B      Jump back word start    (word / WORD)",
    "  0 / ^      Start of line / First non-blank char",
    "  $          End of line",
    "  gg / G     First line / Last line of file",
    "  {N}G       Jump to line N (e.g. 42G)",
    "  <C-d>      Scroll down half a page (cursor stays centered)",
    "  <C-u>      Scroll up half a page   (cursor stays centered)",
    "  <C-f>/<C-b>  Scroll full page down / up",
    "  %          Jump to matching bracket/paren/brace",
    "  *          Search forward for word under cursor",
    "  #          Search backward for word under cursor",
    "  n / N      Next / Previous search match",
    "  ''         Jump back to previous position",
    "  zz         Center screen on cursor",
    "  zt / zb    Scroll so cursor is at Top / Bottom",
    "",
    "  ── TEXT OBJECTS ────────────────────────────────────────────────",
    "  iw / aw    Inner word / A word (incl. surrounding space)",
    "  i\" / a\"    Inside quotes / Including quotes",
    "  i( / a(    Inside parens / Including parens",
    "  i{ / a{    Inside braces / Including braces",
    "  i[ / a[    Inside brackets / Including brackets",
    "  it / at    Inside tag / Including tag (HTML/XML)",
    "  ip / ap    Inner paragraph / A paragraph",
    "  is / as    Inner sentence / A sentence",
    "",
    "  ── OPERATORS (combine with motion or text object) ───────────────",
    "  d{motion}  Delete  (e.g. dw, d$, dip, d3j)",
    "  c{motion}  Change  (delete + insert mode)",
    "  y{motion}  Yank    (copy)",
    "  >{motion}  Indent right",
    "  <{motion}  Indent left",
    "  ={motion}  Auto-indent",
    "  gU{motion} Uppercase",
    "  gu{motion} Lowercase",
    "  g~{motion} Toggle case",
    "",
    "  ── EDITING SHORTCUTS ───────────────────────────────────────────",
    "  x / X      Delete char under cursor / before cursor",
    "  s / S      Substitute char / whole line",
    "  dd / D     Delete line / Delete to end of line",
    "  yy / Y     Yank line / Yank to end of line",
    "  p / P      Paste after cursor / before cursor",
    "  u          Undo",
    "  <C-r>      Redo",
    "  .          Repeat last change",
    "  J          Join line below to current line",
    "  r{char}    Replace single character with {char}",
    "  ~          Toggle case of character under cursor",
    "  >>  /  <<  Indent / Unindent current line",
    "  =G         Auto-indent from cursor to end of file",
    "",
    "  ── SEARCH & REPLACE ────────────────────────────────────────────",
    "  /pattern   Search forward  (n=next, N=prev)",
    "  ?pattern   Search backward",
    "  :%s/old/new/g      Replace all in file",
    "  :%s/old/new/gc     Replace all with confirmation",
    "  :s/old/new/g       Replace all in current line",
    "",
    "  ── MARKS & JUMPS ───────────────────────────────────────────────",
    "  m{a-z}     Set local mark  (m{A-Z} for global mark)",
    "  `{mark}    Jump to exact mark position",
    "  '{mark}    Jump to mark's line start",
    "  <C-o>      Jump to previous location in jump list",
    "  <C-i>      Jump to next location in jump list",
    "",
    "  ── MACROS ──────────────────────────────────────────────────────",
    "  q{a-z}     Start recording macro into register {a-z}",
    "  q          Stop recording macro",
    "  @{a-z}     Replay macro from register {a-z}",
    "  @@         Repeat last macro",
    "  {N}@{a-z}  Replay macro N times",
    "",
    "  ── WINDOWS & SPLITS ────────────────────────────────────────────",
    "  <C-w>s     Horizontal split",
    "  <C-w>v     Vertical split",
    "  <C-w>h/j/k/l   Move between splits",
    "  <C-w>q     Close current split",
    "  <C-w>=     Equalize all split sizes",
    "  <C-w>|     Maximize current split width",
    "  <C-w>_     Maximize current split height",
    "",
    "  ── COMMAND MODE ────────────────────────────────────────────────",
    "  :w         Save file",
    "  :q         Quit",
    "  :wq / :x   Save and quit",
    "  :q!        Quit without saving",
    "  :e {file}  Open file",
    "  :bn / :bp  Next / Previous buffer",
    "  :bd        Delete (close) buffer",
    "  :noh       Clear search highlight",
    "  :set nu    Toggle line numbers",
    "  :!{cmd}    Run shell command",
}

local TUIM_KEYS = {
    "  ╭──────────────────────────────────────────────────────────────╮",
    "  │                ⚙  TUIM CUSTOM KEYBINDINGS                   │",
    "  ╰──────────────────────────────────────────────────────────────╯",
    "  Leader key = <Space>",
    "",
    "  ── TUIM & WORKSPACE ────────────────────────────────────────────",
    "  <Space> e              Toggle File Explorer",
    "  <Space> m t            Toggle IDE / Zen Mode",
    "  <Space> o t            Toggle Bottom Terminal Split",
    "  <Space> o T            Toggle Vertical Terminal Split",
    "  <C-w> s / <C-w> v      Horizontal / Vertical Editor Split",
    "  <C-w> h/j/k/l          Move Between Editor Splits",
    "  <C-w> q                Close Current Editor Split",
    "  <Space> ?              Show Tuim Quickstart Guide",
    "  :TuimOnboarding        Reopen the first-run guide",
    "  <Space> ,              Open Settings",
    "",
    "  ── TTY & KEYBOARD NAVIGATION ───────────────────────────────────",
    "  Ctrl+E                 Toggle and focus File Tree",
    "  Ctrl+T                 Toggle and focus Terminal panel",
    "  <Space> o t / o T      Toggle native bottom / vertical terminal",
    "  F6 / Shift+F6          Next / previous visible workspace region",
    "  F12                    Report bug (opens consent form)",
    "  Terminal panes are also available in Zen mode (F11).",
    "  Terminal copy/paste: drag + Ctrl+C; Ctrl+V / Shift+Insert to paste.",
    "  Alt + Arrow Keys       Resize panels",
    "",
    "  ── TELESCOPE & SEARCH ──────────────────────────────────────────",
    "  F1                     Open native command menu",
    "  <Space> <Space>        Source current file as Vim/Lua config",
    "  <Space> f f            Telescope: Find Files",
    "  <Space> f g / Alt+G    Telescope: Search Project (ripgrep)",
    "  <Space> f r            Telescope: Open Recent Files",
    "  Ctrl+F (IDE)           Find text in the current file",
    "",
    "  ── IDE EDITING (Ctrl+S/N/Q are also workspace defaults) ────────",
    "  Ctrl+S                 Save file",
    "  Ctrl+Q                 Quit Tuim (confirm unsaved changes)",
    "  F1 > Close buffer      Close current buffer",
    "  Ctrl+N                 Open new buffer",
    "  F1 > Switch buffers    Choose an open buffer",
    "  Workspace defaults can be changed in Keyboard shortcuts.",
    "  Ctrl+Z                 Undo",
    "  Ctrl+Y                 Redo",
    "  Ctrl+C                 Copy selection to system clipboard",
    "  Ctrl+X                 Cut selection to system clipboard",
    "  Ctrl+V                 Paste from system clipboard",
    "  Ctrl+A                 Select all text",
    "",
    "  ── PORTED NMUX42 KEYS ──────────────────────────────────────────",
    "  J / K (Visual)         Move selected lines down / up",
    "  J (Normal)             Join lines without moving cursor",
    "  Ctrl+D / Ctrl+U        Scroll half page and keep cursor centered",
    "  n / N                  Next / Prev search match and keep centered",
    "  <Space> p (Visual)     Paste over selection without yanking it",
    "  <Space> d              Delete to black hole (without yanking)",
    "  <Space> s              Prepare substitution on current line",
    "  <Space> x              Make current file executable (chmod +x)",
    "",
    "  ── AI CODING WORKSPACE ─────────────────────────────────────────",
    "  AI activity icon       Agents, Context, and Actions workspace",
    "  1 / 2 / 3              Open Agents / Context / Actions section",
    "  Left / Right           Switch AI workspace section",
    "  Up / Down + Enter      Choose and run an AI workspace item",
    "  <Space> a f            Add current file reference to AI context",
    "  <Space> a c            Add current buffer contents to AI context",
    "  <Space> a s (Visual)   Add selected code and line range to context",
    "  Ctrl+Alt+S/F/C         Send selection / file path / file content",
}

local state = { active_tab = "vim", buf = nil, win = nil }

local function open_bindings_window()
    local width  = math.min(74, vim.o.columns - 4)
    local height = math.floor(vim.o.lines * 0.85)
    local row    = math.floor((vim.o.lines - height) / 2)
    local col    = math.floor((vim.o.columns - width) / 2)

    local border_style = (not vim.g.tuim_zen_mode) and "none" or "rounded"
    local title_str = border_style == "rounded" and "  Tuim Help Reference " or nil

    local buf = vim.api.nvim_create_buf(false, true)
    local win = vim.api.nvim_open_win(buf, true, {
        relative    = "editor",
        width       = width,
        height      = height,
        row         = row,
        col         = col,
        style       = "minimal",
        border      = border_style,
        title       = title_str,
        title_pos   = border_style == "rounded" and "center" or nil,
    })
    return buf, win
end

local function render()
    if not (state.buf and vim.api.nvim_buf_is_valid(state.buf)) then return end

    local tab_vim  = state.active_tab == "vim"
    local tab1_lbl = tab_vim and "●[1] Vim Motions " or "  [1] Vim Motions "
    local tab2_lbl = tab_vim and "  [2] Tuim Keys  " or "●[2] Tuim Keys  "

    local header = {
        "  " .. tab1_lbl .. " │ " .. tab2_lbl,
        "  ────────────────────────────────────────────────────────────",
        "",
    }

    local content = tab_vim and VIM_MOTIONS or TUIM_KEYS
    local lines = {}
    for _, l in ipairs(header) do table.insert(lines, l) end
    for _, l in ipairs(content) do table.insert(lines, l) end
    table.insert(lines, "")
    table.insert(lines, "  [Tab]/[1]/[2] Switch  │  [o] Onboarding  │  [/] Search  │  [q] Close")

    vim.bo[state.buf].modifiable = true
    vim.api.nvim_buf_set_lines(state.buf, 0, -1, false, lines)
    vim.bo[state.buf].modifiable = false
    vim.api.nvim_win_set_cursor(state.win, { 1, 0 })

    -- Highlight header
    local ns = vim.api.nvim_create_namespace("vimbindings_hl")
    vim.api.nvim_buf_clear_namespace(state.buf, ns, 0, -1)
    vim.api.nvim_buf_add_highlight(state.buf, ns, "Title",   0, 0, -1)
    vim.api.nvim_buf_add_highlight(state.buf, ns, "Comment", 1, 0, -1)
    -- Highlight section headers
    for i, l in ipairs(lines) do
        if l:match("^%s+──") then
            vim.api.nvim_buf_add_highlight(state.buf, ns, "Special", i - 1, 0, -1)
        elseif l:match("^%s+╭") or l:match("^%s+│") or l:match("^%s+╰") then
            vim.api.nvim_buf_add_highlight(state.buf, ns, "DiagnosticInfo", i - 1, 0, -1)
        elseif l:match("^%s+%[") and i == #lines then
            vim.api.nvim_buf_add_highlight(state.buf, ns, "Comment", i - 1, 0, -1)
        end
    end
end

_G.open_help_menu = function()
    if vim.g.tuim_gui_onboarding then
        vim.rpcnotify(1, 'tuim_open_help')
        return
    end
    -- Stop insert mode if we are in it
    if vim.fn.mode() == 'i' then vim.cmd("stopinsert") end

    state.buf, state.win = open_bindings_window()
    vim.bo[state.buf].buftype   = "nofile"
    vim.bo[state.buf].bufhidden = "wipe"
    vim.bo[state.buf].swapfile  = false
    vim.bo[state.buf].filetype = "vimbindings"

    local map = function(k, fn, desc)
        vim.keymap.set({"n", "i"}, k, fn, { buffer = state.buf, silent = true, desc = desc })
    end

    map("q",     function() pcall(vim.api.nvim_win_close, state.win, true) end, "Close")
    map("<Esc>", function() pcall(vim.api.nvim_win_close, state.win, true) end, "Close")
    map("<C-c>", function() pcall(vim.api.nvim_win_close, state.win, true) end, "Close")

    map("<Tab>", function()
        state.active_tab = state.active_tab == "vim" and "tuim" or "vim"
        render()
    end, "Switch tab")
    map("1", function() state.active_tab = "vim";  render() end, "Vim Motions tab")
    map("2", function() state.active_tab = "tuim"; render() end, "Tuim Keys tab")
    map("o", function()
        pcall(vim.api.nvim_win_close, state.win, true)
        vim.schedule(function() vim.cmd('TuimOnboarding') end)
    end, "Open onboarding")

    -- Search within the buffer using built-in /
    map("/", function()
        vim.api.nvim_feedkeys("/", "n", false)
    end, "Search in buffer")

    render()
end



        -- Keymap to open
        -- We bind to normal and visual mode as well. But wait, since tuim forces insert mode,
        -- if the user hits <space> in insert mode it types a space. So we should create a user command.
        vim.api.nvim_create_user_command("HelpMenu", _G.open_help_menu, {})
        vim.keymap.set({ "n", "v" }, "<leader>hk", _G.open_help_menu, { desc = "Show Help Menu" })

-- --- FIRST-RUN ONBOARDING --- --
local onboarding = { buf = nil, win = nil }
local onboarding_marker = vim.fn.stdpath('data') .. '/onboarding-complete'

local function complete_onboarding()
    vim.fn.mkdir(vim.fn.fnamemodify(onboarding_marker, ':h'), 'p')
    vim.fn.writefile({ 'completed' }, onboarding_marker)
    if onboarding.win and vim.api.nvim_win_is_valid(onboarding.win) then pcall(vim.api.nvim_win_close, onboarding.win, true) end
end
_G.tuim_onboarding_dismiss = complete_onboarding

_G.tuim_onboarding_choose = function(mode)
    if mode == 'ide' then
        vim.g.tuim_zen_mode = false
        _G.tuim_enable_ide_mode()
    else
        vim.g.tuim_zen_mode = false
        _G.tuim_disable_ide_mode()
    end
    _G.tuim_save_settings()
    complete_onboarding()
end

_G.open_tuim_onboarding = function()
    if vim.g.tuim_gui_onboarding then
        pcall(vim.rpcnotify, 1, 'tuim_open_onboarding')
        return
    end
    if onboarding.win and vim.api.nvim_win_is_valid(onboarding.win) then return end
    if vim.fn.mode() == 'i' then vim.cmd('stopinsert') end
    local width = math.max(1, math.min(76, vim.o.columns - 4))
    local height = math.max(1, math.min(23, vim.o.lines - 4))
    onboarding.buf = vim.api.nvim_create_buf(false, true)
    onboarding.win = vim.api.nvim_open_win(onboarding.buf, true, {
        relative = 'editor', width = width, height = height,
        row = math.max(0, math.floor((vim.o.lines - height) / 2)),
        col = math.max(0, math.floor((vim.o.columns - width) / 2)),
        style = 'minimal', border = 'rounded', title = ' Welcome to Tuim ', title_pos = 'center',
    })
    vim.bo[onboarding.buf].buftype = 'nofile'
    vim.bo[onboarding.buf].bufhidden = 'wipe'
    vim.bo[onboarding.buf].swapfile = false
    vim.bo[onboarding.buf].filetype = 'tuim-onboarding'
    local lines = {
        '  [ Click to close guide ]  or press Enter / Esc / q',
        '  Closing this guide keeps Tuim open and your current mode.',
        '',
        '  Start here',
        '  1. Open a file: Ctrl+E shows files. Click a file to open it.',
        '     Or press Ctrl+N to create a new file.',
        '  2. Edit: in IDE mode, click in the text and start typing.',
        '     In Normal mode, press i to type; Esc stops typing.',
        '  3. Save: Ctrl+S. For a new file, enter a name when asked.',
        '',
        '  Choose your editing style (press a key to start)',
        '  [i] IDE - type right away, like a regular text editor.',
        '  [n] Normal - Vim controls: i to type, Esc for commands.',
        '  You can change this later in Settings > General.',
        '',
        '  A few useful controls',
        '  Ctrl+F Find   Ctrl+Z Undo   Ctrl+T Open / hide terminal',
        '  Mouse: click to focus, drag text to select, right-click to copy.',
        '  F11 hides the panels; press it again to bring them back.',
        '',
        '  [l] Optional code tools (language servers for code suggestions)',
        '  Reopen this guide from Help: press o on the Help page.',
        '  In Normal mode, you can also type :TuimOnboarding then Enter.',
    }
    vim.api.nvim_buf_set_lines(onboarding.buf, 0, -1, false, lines)
    vim.bo[onboarding.buf].modifiable = false
    vim.wo[onboarding.win].wrap = true
    vim.wo[onboarding.win].linebreak = true
    vim.api.nvim_win_set_cursor(onboarding.win, { 1, 0 })
    local ns = vim.api.nvim_create_namespace('tuim_onboarding')
    vim.api.nvim_buf_add_highlight(onboarding.buf, ns, 'Visual', 0, 2, 26)
    for _, line in ipairs({ 4, 11, 16 }) do
        vim.api.nvim_buf_add_highlight(onboarding.buf, ns, 'Title', line - 1, 0, -1)
    end
    local map = function(key, callback, desc)
        vim.keymap.set({ 'n', 'i' }, key, callback, { buffer = onboarding.buf, silent = true, desc = desc })
    end
    map('n', function() _G.tuim_onboarding_choose('normal') end, 'Choose Normal mode')
    map('i', function() _G.tuim_onboarding_choose('ide') end, 'Choose IDE mode')
    map('<CR>', complete_onboarding, 'Close guide and start editing')
    map('<C-c>', complete_onboarding, 'Dismiss onboarding')
    map('<LeftMouse>', function()
        local mouse = vim.fn.getmousepos()
        if mouse.winid == onboarding.win and mouse.line == 1 then
            complete_onboarding()
        else
            vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes('<LeftMouse>', true, false, true), 'n', false)
        end
    end, 'Click to close guide')
    map('q', complete_onboarding, 'Dismiss onboarding')
    map('<Esc>', complete_onboarding, 'Dismiss onboarding')
    map('l', function()
        complete_onboarding()
        -- Use Tuim's native package panel, as Settings > Plugins does.
        -- Opening Mason's Neovim float bypasses that UI and fails when the
        -- plugin command has not been registered yet.
        local ok = pcall(vim.rpcnotify, 1, 'tuim_open_language_tools')
        if not ok then
            local opened, err = pcall(vim.cmd, 'Mason')
            if not opened then
                vim.notify('Language tools are unavailable. Open Settings > Plugins to enable Mason. ' .. tostring(err), vim.log.levels.WARN)
            end
        end
    end, 'Review language servers')
end

vim.api.nvim_create_user_command('TuimOnboarding', _G.open_tuim_onboarding, {})
if not vim.g.tuim_gui_onboarding and vim.env.TUIM_SKIP_ONBOARDING ~= '1' and vim.uv.fs_stat(onboarding_marker) == nil then
    vim.schedule(_G.open_tuim_onboarding)
end

_G.last_ai_job_id = nil
_G.last_ai_command = nil
_G.last_ai_buf = nil
_G.last_ai_win = nil
_G.last_ai_source_win = nil
_G.ai_generation = 0
_G.ai_ready_at = 0
_G.tuim_ai_sessions = {}

local ai_context_limit = 64 * 1024

local function ai_notify_status(command, state)
    pcall(vim.rpcnotify, 1, "tuim_ai_status", command or "", state, command == _G.last_ai_command)
end

local function ai_sanitize(text)
    text = tostring(text or ""):gsub("%z", ""):gsub("\27", "")
    if #text > ai_context_limit then
        text = text:sub(1, ai_context_limit) .. "\n\n[Context truncated by Tuim at 64 KiB]"
    end
    return text
end

function _G.GetActiveAIJob()
    local job_id = _G.last_ai_job_id
    if not job_id then return nil end
    local ok, res = pcall(vim.fn.jobwait, { job_id }, 0)
    if ok and res and res[1] == -1 then return job_id end
    _G.last_ai_job_id = nil
    ai_notify_status(_G.last_ai_command, "stopped")
    return nil
end

local ai_agent_preference = { "codex", "claude", "gemini", "opencode", "agy", "copilot" }

local function ai_notice(level, message)
    _G.tuim_native_notice(level, message)
end

local function require_ai_job()
    local job_id = _G.GetActiveAIJob()
    if job_id then return job_id end
    local command = _G.last_ai_command
    if not command or vim.fn.executable(command) == 0 then
        command = nil
        for _, candidate in ipairs(ai_agent_preference) do
            if vim.fn.executable(candidate) == 1 then command = candidate; break end
        end
    end
    if not command then
        ai_notice("warning", "Install Codex, Claude Code, Gemini, OpenCode, Antigravity, or Copilot to use AI actions.")
        return nil
    end
    _G.OpenAITerminal(command)
    return _G.GetActiveAIJob()
end

local function send_ai_text(text, submit)
    local job_id = require_ai_job()
    if not job_id then return false end
    text = ai_sanitize(text)
    local generation = _G.ai_generation
    local payload = "\27[200~" .. text .. "\27[201~" .. (submit and "\r" or "")
    local function deliver()
        if generation ~= _G.ai_generation or _G.GetActiveAIJob() ~= job_id then
            ai_notice("warning", "The AI session stopped before the context could be delivered.")
            return
        end
        vim.fn.chansend(job_id, payload)
    end

    -- A freshly launched full-screen agent needs a moment to enable its input
    -- parser. Queue the first prompt instead of letting an initializing process
    -- consume or discard bracketed-paste control sequences.
    local delay = math.max(0, (_G.ai_ready_at or 0) - vim.uv.now())
    if delay > 0 then vim.defer_fn(deliver, delay) else deliver() end
    return true
end

local function context_block(kind, metadata, body)
    return string.format("<tuim_context type=%q %s>\n%s\n</tuim_context>\n", kind, metadata or "", ai_sanitize(body))
end

local function source_buffer()
    if _G.last_ai_source_win and vim.api.nvim_win_is_valid(_G.last_ai_source_win) then
        local buf = vim.api.nvim_win_get_buf(_G.last_ai_source_win)
        if vim.bo[buf].buftype == "" then return buf end
    end
    local current = vim.api.nvim_get_current_buf()
    if vim.bo[current].buftype == "" then
        -- Remember the editor window before an AI action changes focus. This is
        -- also needed to leave Visual mode and finalize the '< and '> marks.
        _G.last_ai_source_win = vim.api.nvim_get_current_win()
        return current
    end
    for _, win in ipairs(vim.api.nvim_list_wins()) do
        local buf = vim.api.nvim_win_get_buf(win)
        if vim.bo[buf].buftype == "" then
            _G.last_ai_source_win = win
            return buf
        end
    end
    return nil
end

local function current_file()
    local buf = source_buffer()
    if not buf then return nil end
    local path = vim.api.nvim_buf_get_name(buf)
    if path == "" then return nil end
    return path
end

local function selected_text()
    local buf = source_buffer()
    if not buf then return nil end
    local mode = vim.api.nvim_get_mode().mode
    if (mode:sub(1, 1) == "v" or mode:sub(1, 1) == "V" or mode == "\22") and
        _G.last_ai_source_win and vim.api.nvim_win_is_valid(_G.last_ai_source_win) then
        vim.api.nvim_win_call(_G.last_ai_source_win, function() vim.cmd("normal! \27") end)
    end
    local first = vim.api.nvim_buf_get_mark(buf, "<")
    local last = vim.api.nvim_buf_get_mark(buf, ">")
    if not first or not last or first[1] == 0 or last[1] == 0 then return nil end
    local lines = vim.api.nvim_buf_get_lines(buf, first[1] - 1, last[1], false)
    if #lines == 0 then return nil end
    if vim.fn.visualmode() == "v" then
        lines[#lines] = lines[#lines]:sub(1, last[2] + 1)
        lines[1] = lines[1]:sub(first[2] + 1)
    end
    return table.concat(lines, "\n"), first[1], last[1]
end

local function diagnostics_text()
    local buf = source_buffer()
    if not buf then return "No source buffer is active." end
    local diagnostics = vim.diagnostic.get(buf)
    if #diagnostics == 0 then return "No diagnostics in the current file." end
    local severity = vim.diagnostic.severity
    local names = { [severity.ERROR] = "ERROR", [severity.WARN] = "WARN", [severity.INFO] = "INFO", [severity.HINT] = "HINT" }
    local lines = {}
    for _, diagnostic in ipairs(diagnostics) do
        table.insert(lines, string.format("%s %d:%d %s", names[diagnostic.severity] or "DIAG", diagnostic.lnum + 1, diagnostic.col + 1, diagnostic.message:gsub("\n", " ")))
    end
    return table.concat(lines, "\n")
end

local function git_diff_text()
    local diff = vim.fn.system({ "git", "diff", "--no-ext-diff", "HEAD" })
    if vim.v.shell_error ~= 0 then diff = vim.fn.system({ "git", "diff", "--no-ext-diff" }) end
    if diff == "" then return "Working tree has no tracked changes." end
    return diff
end

function _G.SendSelectionToAI()
    local text, first, last = selected_text()
    if not text then
        ai_notice("warning", "Select some code first, then open the right-click menu.")
        return
    end
    local path = current_file() or "[No Name]"
    if send_ai_text(context_block("selection", string.format("file=%q lines=%q", path, first .. "-" .. last), text), false) then
        vim.notify("Selection added to AI context", vim.log.levels.INFO)
    end
end

function _G.SendFilePathToAI()
    local path = current_file()
    if not path then
        vim.notify("The current buffer has no file path.", vim.log.levels.WARN)
        return
    end
    if send_ai_text(context_block("file", string.format("path=%q", path), "Use this file as context."), false) then
        vim.notify("File reference added to AI context", vim.log.levels.INFO)
    end
end

function _G.SendFileContentToAI()
    local path = current_file() or "[No Name]"
    local buf = source_buffer()
    if not buf then vim.notify("No source buffer is active.", vim.log.levels.WARN); return end
    local text = table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), "\n")
    if send_ai_text(context_block("buffer", string.format("file=%q", path), text), false) then
        vim.notify("Buffer contents added to AI context", vim.log.levels.INFO)
    end
end

function _G.SendDiagnosticsToAI()
    local path = current_file() or "[No Name]"
    if send_ai_text(context_block("diagnostics", string.format("file=%q", path), diagnostics_text()), false) then
        vim.notify("Diagnostics added to AI context", vim.log.levels.INFO)
    end
end

function _G.SendGitDiffToAI()
    if send_ai_text(context_block("git_diff", "", git_diff_text()), false) then
        vim.notify("Git diff added to AI context", vim.log.levels.INFO)
    end
end

function _G.RunAIAction(action)
    local path = current_file() or "[No Name]"
    local prompt
    if action == "fix_diagnostics" then
        prompt = "Fix the diagnostics in " .. path .. ". Inspect the surrounding code, make the smallest correct edits, and run relevant checks.\n" .. context_block("diagnostics", string.format("file=%q", path), diagnostics_text())
    elseif action == "explain_selection" then
        local text, first, last = selected_text()
        if not text then ai_notice("warning", "Select some code first, then choose Explain with AI."); return end
        prompt = "Explain this code clearly, including intent, important control flow, and likely pitfalls.\n" .. context_block("selection", string.format("file=%q lines=%q", path, first .. "-" .. last), text)
    elseif action == "fix_selection" then
        local text, first, last = selected_text()
        if not text then ai_notice("warning", "Select some code first, then choose Fix / Improve with AI."); return end
        prompt = "Fix or improve the selected code in " .. path .. ". Preserve its intended behavior, follow the surrounding conventions, make the smallest useful edit, and run relevant checks.\n" .. context_block("selection", string.format("file=%q lines=%q", path, first .. "-" .. last), text)
    elseif action == "write_tests" then
        prompt = "Write or improve focused tests for " .. path .. ". Follow the repository's existing test style and run the relevant test command."
    elseif action == "review_changes" then
        prompt = "Review the current working-tree changes for correctness, regressions, security issues, and missing tests. Report findings by severity before proposing edits.\n" .. context_block("git_diff", "", git_diff_text())
    elseif action == "implement_todo" then
        local buf = source_buffer()
        if not buf then vim.notify("No source buffer is active.", vim.log.levels.WARN); return end
        local row = 1
        if _G.last_ai_source_win and vim.api.nvim_win_is_valid(_G.last_ai_source_win) then
            row = vim.api.nvim_win_get_cursor(_G.last_ai_source_win)[1]
        end
        local first = math.max(0, row - 8)
        local last = math.min(vim.api.nvim_buf_line_count(buf), row + 7)
        local nearby = table.concat(vim.api.nvim_buf_get_lines(buf, first, last, false), "\n")
        prompt = "Implement the TODO or unfinished behavior near the cursor in " .. path .. ". Preserve local conventions and run relevant tests.\n" .. context_block("near_cursor", string.format("file=%q lines=%q", path, (first + 1) .. "-" .. last), nearby)
    else
        ai_notice("error", "Unknown AI action: " .. tostring(action))
        return
    end
    if send_ai_text(prompt, true) then vim.schedule(_G.FocusAITerminal) end
end

vim.keymap.set("v", "<leader>as", _G.SendSelectionToAI, { desc = "Send selected text to AI terminal" })
vim.keymap.set("n", "<leader>af", _G.SendFilePathToAI, { desc = "Send current file path to AI terminal" })
vim.keymap.set("n", "<leader>ac", _G.SendFileContentToAI, { desc = "Send entire file content to AI terminal" })


vim.keymap.set({ "n", "v", "i" }, "<C-M-s>", _G.SendSelectionToAI, { desc = "Send selected text to AI terminal" })
vim.keymap.set({ "n", "v", "i" }, "<C-M-f>", _G.SendFilePathToAI, { desc = "Send current file path to AI terminal" })
vim.keymap.set({ "n", "v", "i" }, "<C-M-c>", _G.SendFileContentToAI, { desc = "Send entire file content to AI terminal" })

function _G.NotifyAIMissing(cmd)
    ai_notice("warning", "AI agent '" .. cmd .. "' is not installed or not available on PATH.")
end

function _G.OpenTerminalRight()
    _G.tuim_close_floating_windows()
    vim.rpcnotify(1, "tuim_open_terminal_right")
end

local function configure_ai_winbar(win, command)
    if not win or not vim.api.nvim_win_is_valid(win) then return end
    -- Tuim draws split-owned tabs in its native top strip.
    vim.wo[win].winbar = ""
end

function _G.FocusAITerminal()
    if not require_ai_job() then return end
    if _G.last_ai_win and vim.api.nvim_win_is_valid(_G.last_ai_win) then
        vim.api.nvim_set_current_win(_G.last_ai_win)
        vim.cmd("startinsert")
        return
    end
    if _G.last_ai_buf and vim.api.nvim_buf_is_valid(_G.last_ai_buf) then
        vim.cmd("botright vsplit")
        vim.cmd("wincmd L")
        vim.api.nvim_win_set_buf(0, _G.last_ai_buf)
        _G.last_ai_win = vim.api.nvim_get_current_win()
        configure_ai_winbar(_G.last_ai_win, _G.last_ai_command or "agent")
        vim.cmd("startinsert")
    end
end

function _G.StopAITerminal()
    local job_id = _G.GetActiveAIJob()
    if not job_id then return end
    _G.ai_generation = _G.ai_generation + 1
    _G.last_ai_job_id = nil
    local session = _G.tuim_ai_sessions[_G.last_ai_command]
    if session then session.job = nil end
    pcall(vim.fn.jobstop, job_id)
    ai_notify_status(_G.last_ai_command, "stopped")
    ai_notice("info", "AI session stopped")
end

function _G.CloseAITerminal()
    local job_id = _G.GetActiveAIJob()
    local win = _G.last_ai_win
    local buf = _G.last_ai_buf
    local source_win = _G.last_ai_source_win

    -- Invalidate callbacks before stopping the job so a stale on_exit cannot
    -- overwrite the state of a newly opened agent session.
    _G.ai_generation = _G.ai_generation + 1
    _G.last_ai_job_id = nil
    _G.last_ai_win = nil
    _G.last_ai_buf = nil
    _G.ai_ready_at = 0
    _G.tuim_ai_sessions[_G.last_ai_command or ''] = nil

    if job_id then pcall(vim.fn.jobstop, job_id) end
    if win and vim.api.nvim_win_is_valid(win) then pcall(vim.api.nvim_win_close, win, true) end
    if buf and vim.api.nvim_buf_is_valid(buf) then pcall(vim.api.nvim_buf_delete, buf, { force = true }) end
    ai_notify_status(_G.last_ai_command, "stopped")

    if source_win and vim.api.nvim_win_is_valid(source_win) then
        pcall(vim.api.nvim_set_current_win, source_win)
    end
    ai_notice("info", "AI panel closed")
end

function _G.TuimCloseAIWinbar()
    _G.CloseAITerminal()
end

function _G.RestartAITerminal()
    local command = _G.last_ai_command
    if not command then
        ai_notice("warning", "No AI session to restart.")
        return
    end
    _G.StopAITerminal()
    vim.schedule(function() _G.OpenAITerminal(command) end)
end

function _G.OpenAITerminal(cmd)
    if vim.fn.executable(cmd) == 0 then _G.NotifyAIMissing(cmd); return end
    local session = _G.tuim_ai_sessions[cmd]
    if session and session.job and vim.fn.jobwait({ session.job }, 0)[1] ~= -1 then session.job = nil end
    local current_win = vim.api.nvim_get_current_win()
    local current_buf = vim.api.nvim_get_current_buf()
    if vim.bo[current_buf].buftype == "" then _G.last_ai_source_win = current_win end
    if _G.last_ai_win and vim.api.nvim_win_is_valid(_G.last_ai_win) then
        vim.api.nvim_set_current_win(_G.last_ai_win)
    else
        vim.cmd("botright vsplit")
        vim.cmd("wincmd L")
    end
    _G.ai_generation = _G.ai_generation + 1
    local generation = _G.ai_generation
    _G.last_ai_command = cmd
    _G.last_ai_win = vim.api.nvim_get_current_win()
    if session and session.job and vim.api.nvim_buf_is_valid(session.buf) then
        _G.last_ai_job_id, _G.last_ai_buf = session.job, session.buf
        _G.ai_ready_at = session.ready_at
        vim.api.nvim_win_set_buf(0, session.buf)
        configure_ai_winbar(_G.last_ai_win, cmd)
        ai_notify_status(cmd, 'running')
        vim.cmd('startinsert')
        return
    end
    if session and vim.api.nvim_buf_is_valid(session.buf) then
        pcall(vim.api.nvim_buf_delete, session.buf, { force = true })
    end
    vim.cmd('enew')
    session = { buf = vim.api.nvim_get_current_buf(), ready_at = vim.uv.now() + 900 }
    _G.tuim_ai_sessions[cmd] = session
    vim.b[session.buf].tuim_ai = true
    -- Launch the agent directly. Going through an interactive shell creates a
    -- race where the first AI prompt can be pasted into the shell before its
    -- `exec` command has replaced it with the agent.
    local job_id = vim.fn.termopen({ cmd }, {
        on_exit = function()
            if _G.tuim_ai_sessions[cmd] ~= session then return end
            session.job = nil
            if _G.last_ai_command == cmd then _G.last_ai_job_id = nil end
            ai_notify_status(cmd, "stopped")
        end,
    })
    _G.last_ai_job_id = job_id
    session.job = job_id > 0 and job_id or nil
    _G.last_ai_command = cmd
    _G.ai_ready_at = session.ready_at
    _G.last_ai_buf = vim.api.nvim_get_current_buf()
    _G.last_ai_win = vim.api.nvim_get_current_win()
    vim.bo[_G.last_ai_buf].bufhidden = "hide"
    vim.api.nvim_buf_set_name(_G.last_ai_buf, string.format("AI: %s [%d]", cmd, generation))
    configure_ai_winbar(_G.last_ai_win, cmd)
    ai_notify_status(cmd, session.job and "running" or "stopped")
    vim.cmd("startinsert")
end

-- Tuim renders its own native editor context menu, so Neovim must not also
-- open or process a second right-click menu in the embedded UI.
for _, mode in ipairs({ 'n', 'v', 'i', 's', 'c' }) do
    vim.keymap.set(mode, '<RightMouse>', '<Nop>', { silent = true })
    vim.keymap.set(mode, '<RightRelease>', '<Nop>', { silent = true })
    vim.keymap.set(mode, '<RightDrag>', '<Nop>', { silent = true })
    vim.keymap.set(mode, '<2-RightMouse>', '<Nop>', { silent = true })
    vim.keymap.set(mode, '<3-RightMouse>', '<Nop>', { silent = true })
    vim.keymap.set(mode, '<4-RightMouse>', '<Nop>', { silent = true })
end

-- Tuim owns the visual tab strip, but Neovim remains the source of truth for
-- buffers. Notify the frontend after every relevant buffer lifecycle event,
-- keeping the active file tab stable while floating pickers (Telescope, etc.)
-- hold focus.
local last_active_listed_buf = nil
local last_buffers_sig = nil
local last_cursor_line = nil
local last_cursor_col = nil

local function tuim_notify_buffers()
    local buffers = {}
    local listed_set = {}
    local sig_parts = {}
    for _, info in ipairs(vim.fn.getbufinfo({ buflisted = 1 })) do
        -- Split-owned AI terminals have their own winbar tab and should not
        -- masquerade as files in Tuim's global file tab strip.
        if not vim.b[info.bufnr].tuim_ai then
            local name = info.name or ""
            local rel = name == "" and "" or vim.fn.fnamemodify(name, ":.")
            local changed = info.changed == 1
            local ft = vim.bo[info.bufnr].filetype or ""
            listed_set[info.bufnr] = true
            table.insert(buffers, {
                bufnr = info.bufnr,
                name = name,
                relative_name = rel,
                filetype = ft,
                changed = changed,
            })
            table.insert(sig_parts, string.format("%d:%s:%s:%d", info.bufnr, name, ft, changed and 1 or 0))
        end
    end

    local cur_win = vim.api.nvim_get_current_win()
    local cur_buf = vim.api.nvim_get_current_buf()
    local cur_is_float = false
    if vim.api.nvim_win_is_valid(cur_win) then
        local ok_cfg, cfg = pcall(vim.api.nvim_win_get_config, cur_win)
        cur_is_float = ok_cfg and cfg.relative ~= ""
    end

    local active_buf = cur_buf
    if not cur_is_float and listed_set[cur_buf] and vim.bo[cur_buf].buftype == "" then
        last_active_listed_buf = cur_buf
        active_buf = cur_buf
    elseif last_active_listed_buf and listed_set[last_active_listed_buf] then
        active_buf = last_active_listed_buf
    else
        local primary = tuim_primary_editor_win()
        if primary and vim.api.nvim_win_is_valid(primary) then
            local pbuf = vim.api.nvim_win_get_buf(primary)
            if listed_set[pbuf] then
                active_buf = pbuf
                last_active_listed_buf = pbuf
            end
        end
    end

    local sig = tostring(active_buf) .. "|" .. table.concat(sig_parts, ";")
    if sig == last_buffers_sig then return end
    last_buffers_sig = sig
    pcall(vim.rpcnotify, 1, "tuim_buffers", buffers, active_buf)
end
_G.tuim_notify_buffers = tuim_notify_buffers

local function tuim_notify_cursor()
    local mode_info = vim.api.nvim_get_mode()
    local m = mode_info.mode or ""
    if mode_info.blocking or m:sub(1, 1) == "c" or m:sub(1, 1) == "r" then
        return
    end
    local win = vim.api.nvim_get_current_win()
    if not vim.api.nvim_win_is_valid(win) then return end
    local ok_cfg, cfg = pcall(vim.api.nvim_win_get_config, win)
    if not ok_cfg or cfg.relative ~= "" then return end
    local buf = vim.api.nvim_win_get_buf(win)
    if not vim.api.nvim_buf_is_valid(buf) then return end
    local bt = vim.bo[buf].buftype
    if bt ~= "" and bt ~= "acwrite" then return end
    local ok_line, line = pcall(vim.fn.line, ".")
    local ok_col, col = pcall(vim.fn.charcol, ".")
    if not ok_line or not ok_col then return end
    line = math.max(1, tonumber(line) or 1)
    col = math.max(1, tonumber(col) or 1)
    if line == last_cursor_line and col == last_cursor_col then return end
    last_cursor_line = line
    last_cursor_col = col
    pcall(vim.rpcnotify, 1, "tuim_cursor", line, col)
end
_G.tuim_notify_cursor = tuim_notify_cursor

local buf_sync_group = vim.api.nvim_create_augroup("TuimBufferSync", { clear = true })
vim.api.nvim_create_autocmd({ "BufAdd", "BufDelete", "BufWipeout", "BufEnter", "FileType", "TextChanged", "TextChangedI", "CmdlineLeave" }, {
    group = buf_sync_group,
    callback = function()
        vim.schedule(tuim_notify_buffers)
    end,
})
vim.api.nvim_create_autocmd({ "BufModifiedSet", "BufFilePost", "BufWritePost" }, {
    group = buf_sync_group,
    callback = function(ev)
        -- Send immediately so even commands run via <C-o>:saveas or :%s update
        -- tabs, dirty dot, title, and Explorer without waiting for idle ticks.
        pcall(tuim_notify_buffers)
        if ev.event == "BufFilePost" or ev.event == "BufWritePost" then
            if _G.tuim_notify_win_positions then pcall(_G.tuim_notify_win_positions) end
            local saved_path = vim.api.nvim_buf_get_name(ev.buf)
            if saved_path ~= "" then
                pcall(vim.rpcnotify, 1, "tuim_file_saved", saved_path)
            end
        end
        vim.schedule(tuim_notify_buffers)
    end,
})
vim.api.nvim_create_autocmd({ "CursorMoved", "CursorMovedI", "BufEnter", "WinEnter", "ModeChanged", "TextChanged", "TextChangedI", "CmdlineLeave" }, {
    group = vim.api.nvim_create_augroup("TuimCursorSync", { clear = true }),
    callback = function()
        pcall(tuim_notify_cursor)
    end,
})
vim.schedule(tuim_notify_buffers)
vim.schedule(tuim_notify_cursor)

-- --- USER SELF-CONFIGURATION (~/.config/tuim/) --- --
local USER_INIT_TEMPLATE = [[-- Personal Gtuim / Neovim configuration
-- Loaded automatically after plugins and settings are initialized.
-- Reload live at any time with :TuimReloadConfig (or F1 > Config: Reload user init.lua).
--
-- Examples:
--   vim.opt.scrolloff = 6
--   vim.opt.wrap = false
--   vim.keymap.set("n", "<leader>w", "<cmd>w<cr>", { desc = "Save file" })
--   vim.cmd.colorscheme("gruber-darker")
]]

local USER_PLUGINS_TEMPLATE = [[-- Personal Gtuim / Lazy.nvim plugin specs
-- Return a lazy.nvim spec table (or list of specs) to add new plugins or
-- customize bundled/marketplace plugins.
-- Per-plugin files can also be placed in ~/.config/tuim/plugins/*.lua.
-- Apply plugin changes via F1 > Plugins: Restart Neovim session.
--
-- Examples:
-- return {
--   { "lewis6991/gitsigns.nvim", opts = {} },
--   { "windwp/nvim-autopairs", event = "InsertEnter", opts = {} },
--   {
--     "blazkowolf/gruber-darker.nvim",
--     lazy = false,
--     priority = 1000,
--     opts = { bold = true, italic = { strings = false } },
--   },
-- }
return {}
]]

local function ensure_user_config_file(path, template)
    vim.fn.mkdir(vim.fn.fnamemodify(path, ":h"), "p")
    if vim.fn.filereadable(path) ~= 1 then
        local f = io.open(path, "w")
        if f then
            f:write(template)
            f:close()
        end
    end
    return path
end

_G.tuim_edit_user_init = function()
    local path = ensure_user_config_file(user_config_dir .. "/init.lua", USER_INIT_TEMPLATE)
    vim.cmd("edit " .. vim.fn.fnameescape(path))
end

_G.tuim_edit_user_plugins = function()
    local path = ensure_user_config_file(user_config_dir .. "/plugins.lua", USER_PLUGINS_TEMPLATE)
    vim.cmd("edit " .. vim.fn.fnameescape(path))
end

_G.tuim_reload_user_config = function(silent)
    local user_init = user_config_dir .. "/init.lua"
    if vim.fn.filereadable(user_init) ~= 1 then
        if not silent then
            _G.tuim_native_notice("info", "No user init.lua yet. Run :TuimEditInit or Settings > Plugins to create " .. user_init)
        end
        return false
    end
    local chunk, load_err = loadfile(user_init)
    if not chunk then
        _G.tuim_native_notice("error", "Syntax error in " .. user_init .. ": " .. tostring(load_err))
        return false
    end
    local ok, err = pcall(chunk)
    if not ok then
        _G.tuim_native_notice("error", "Error in " .. user_init .. ": " .. tostring(err))
        return false
    end
    if not silent then
        _G.tuim_native_notice("info", "Reloaded " .. user_init)
    end
    return true
end

_G.tuim_plugin_health = function()
    local total, loaded, custom, needs_cfg = 0, 0, 0, {}
    for _, entry in pairs(plugin_inventory) do
        if not entry.removed and entry.enabled then
            total = total + 1
            if entry.loaded then loaded = loaded + 1 end
            if entry.config_path and entry.config_path ~= "" then custom = custom + 1 end
            if entry.auto_init == "needs_config" then table.insert(needs_cfg, entry.name) end
        end
    end
    local msg = string.format("Plugins: %d/%d loaded, %d customized (config dir: %s)", loaded, total, custom, user_config_dir)
    if #needs_cfg > 0 then
        table.sort(needs_cfg)
        msg = msg .. " | Needs config: " .. table.concat(needs_cfg, ", ")
    end
    _G.tuim_native_notice(#needs_cfg > 0 and "warning" or "info", msg)
end

vim.api.nvim_create_user_command("TuimEditInit", _G.tuim_edit_user_init, { desc = "Edit ~/.config/tuim/init.lua" })
vim.api.nvim_create_user_command("TuimEditPlugins", _G.tuim_edit_user_plugins, { desc = "Edit ~/.config/tuim/plugins.lua" })
vim.api.nvim_create_user_command("TuimReloadConfig", function() _G.tuim_reload_user_config(false) end, { desc = "Reload ~/.config/tuim/init.lua" })
vim.api.nvim_create_user_command("TuimPluginHealth", _G.tuim_plugin_health, { desc = "Show Tuim plugin status summary" })

-- Load ~/.config/tuim/init.lua automatically at end of startup.
_G.tuim_reload_user_config(true)
