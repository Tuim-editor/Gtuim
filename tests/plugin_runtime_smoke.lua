-- Offline integration test. The Python runner supplies isolated XDG directories.
-- Lazy is mocked so no network, real user plugins, or setup hooks are executed.
local root = assert(vim.env.GTUIM_TEST_ROOT)
local scenario = vim.env.GTUIM_TEST_SCENARIO or "automatic"
local data = vim.fn.stdpath("data")
local config = vim.env.XDG_CONFIG_HOME .. "/tuim"
local bootstrap_opts
if scenario == "bootstrap_timeout" then
    -- Simulate a timed-out clone, never starting a real process or using the network.
    vim.system = function(command, opts)
        assert(command[1] == "git" and command[2] == "clone", "unexpected bootstrap command")
        bootstrap_opts = opts
        return { wait = function() return { code = 124, stderr = "fixture clone timeout" } end }
    end
else
    vim.fn.mkdir(data .. "/lazy/lazy.nvim", "p")
end
vim.fn.mkdir(data .. "/lazy/smoke.nvim/lua/smoke", "p")
vim.fn.mkdir(config .. "/plugins", "p")
local module_source = 'return { setup = function(opts) _G.smoke_setup = opts end }'
if scenario == "setup_error" then
    module_source = 'return { setup = function() error("fixture setup failure") end }'
elseif scenario == "require_error" then
    module_source = 'error("fixture module load failure")'
end
vim.fn.writefile({ module_source }, data .. "/lazy/smoke.nvim/lua/smoke/init.lua")
vim.fn.writefile({ 'return { "fixture/smoke.nvim" }' }, config .. "/plugins.lua")
if scenario == "override" then
    vim.fn.writefile({ 'return { opts = { greeting = "configured" } }' },
        config .. "/plugins/fixture_smoke.nvim.lua")
elseif scenario == "bad_config" then
    vim.fn.writefile({ 'return { invalid lua syntax' },
        config .. "/plugins/fixture_smoke.nvim.lua")
end
if scenario == "recipe_options" or scenario == "recipe_override_precedence" then
    vim.fn.mkdir(data .. "/lazy/gruber-darker.nvim/lua", "p")
    vim.fn.mkdir(data .. "/lazy/gruber-darker.nvim/colors", "p")
    vim.fn.writefile({ 'return { setup = function(opts) _G.gruber_setup = opts end, load = function() end }' },
        data .. "/lazy/gruber-darker.nvim/lua/gruber-darker.lua")
    vim.fn.writefile({ '-- fixture colorscheme' },
        data .. "/lazy/gruber-darker.nvim/colors/gruber-darker.lua")
    vim.fn.writefile({ '["blazkowolf/gruber-darker.nvim"]' }, data .. "/user_plugins.json")
    vim.fn.writefile({ '{"blazkowolf/gruber-darker.nvim": {"bold": false, "italic.strings": false}}' },
        data .. "/recipe_options.json")
    vim.fn.writefile({ '["blazkowolf/gruber-darker.nvim"]' }, data .. "/plugin_pending_restart.json")
    if scenario == "recipe_override_precedence" then
        vim.fn.writefile({ 'return { opts = { bold = true, custom_flag = 42 } }' },
            config .. "/plugins/blazkowolf_gruber-darker.nvim.lua")
        vim.fn.writefile({ '{"blazkowolf/gruber-darker.nvim": "abcdef1234567"}' },
            data .. "/plugin_pins.json")
        -- Also test explicit Lua tag override without plugin_pins.json on Comment.nvim.
        vim.fn.mkdir(data .. "/lazy/Comment.nvim/lua", "p")
        vim.fn.writefile({ 'return { setup = function(opts) _G.comment_setup = opts end }' },
            data .. "/lazy/Comment.nvim/lua/Comment.lua")
        vim.fn.writefile({ '["blazkowolf/gruber-darker.nvim", "numToStr/Comment.nvim"]' },
            data .. "/user_plugins.json")
        vim.fn.writefile({ 'return { tag = "v0.8.1-custom", opts = { padding = false } }' },
            config .. "/plugins/numToStr_Comment.nvim.lua")
    end
elseif scenario == "recipe_verify_fail" then
    vim.fn.mkdir(data .. "/lazy/gitsigns.nvim/lua", "p")
    -- Deliberately omit attach() so non-destructive verification catches it.
    vim.fn.writefile({ 'return { setup = function(opts) _G.gitsigns_setup = opts end }' },
        data .. "/lazy/gitsigns.nvim/lua/gitsigns.lua")
    vim.fn.writefile({ '["lewis6991/gitsigns.nvim"]' }, data .. "/user_plugins.json")
elseif scenario == "unmatched_module_and_theme_sync" then
    -- Plugin whose Lua module name does not match any of the first 5 candidates in
    -- detect_plugin_lua_module(): exercises full candidates iteration so a multi-return
    -- gsub count in the table literal would crash startup before lazy.setup().
    vim.fn.mkdir(data .. "/lazy/odd-plugin.nvim/lua", "p")
    vim.fn.writefile({ 'return { setup = function() end }' },
        data .. "/lazy/odd-plugin.nvim/lua/unrelated_mod.lua")
    vim.fn.mkdir(data .. "/lazy/gruber-darker.nvim/lua", "p")
    vim.fn.mkdir(data .. "/lazy/gruber-darker.nvim/colors", "p")
    vim.fn.writefile({ 'return { setup = function(opts) _G.gruber_setup = opts end }' },
        data .. "/lazy/gruber-darker.nvim/lua/gruber-darker.lua")
    vim.fn.writefile({
        'vim.cmd("highlight clear")',
        'vim.g.colors_name = "gruber-darker"',
        'vim.api.nvim_set_hl(0, "Normal", { fg = "#e4e4e4", bg = "#181818" })',
        'vim.api.nvim_set_hl(0, "Function", { fg = "#96a6c8" })',
    }, data .. "/lazy/gruber-darker.nvim/colors/gruber-darker.lua")
    vim.fn.mkdir(config .. "/colors", "p")
    vim.fn.writefile({
        'vim.cmd("highlight clear")',
        'vim.g.colors_name = "local-custom"',
        'vim.api.nvim_set_hl(0, "Normal", { fg = "#eeeeee", bg = "#123456" })',
    }, config .. "/colors/local-custom.lua")
    vim.fn.writefile({ '["fixture/odd-plugin.nvim", "blazkowolf/gruber-darker.nvim"]' },
        data .. "/user_plugins.json")
    vim.fn.writefile({ '{"version": 1, "theme": "gruber-darker"}' }, data .. "/settings.json")
    -- Remove user_config_dir from rtp to simulate `nvim --clean` startup.
    vim.opt.rtp:remove(config)
    package.loaded["vscode"] = {
        setup = function() end,
        load = function()
            vim.g.colors_name = "vscode"
            vim.api.nvim_set_hl(0, "Normal", { fg = "#cccccc", bg = "#1f1f1f" })
            vim.api.nvim_set_hl(0, "TuimAccent", { fg = "#0078d4" })
        end,
    }
end
local notices = {}
local theme_events = {}
vim.rpcnotify = function(_, event, a, b)
    if event == "tuim_notice" then table.insert(notices, { a, b }) end
    if event == "tuim_theme_changed" then table.insert(theme_events, a) end
end
local captured
local lazy_config = { plugins = {}, to_clean = {} }
package.loaded["lazy.core.config"] = lazy_config
package.loaded["lazy"] = {
    setup = function(specs)
        captured = specs
        for _, spec in ipairs(specs) do
            local repo = spec[1]
            if repo == "fixture/smoke.nvim" or repo == "fixture/odd-plugin.nvim" or repo == "blazkowolf/gruber-darker.nvim" or repo == "lewis6991/gitsigns.nvim" or repo == "numToStr/Comment.nvim" then
                local name = repo:match("([^/]+)$")
                local plugin = { [1] = repo, name = name, dir = data .. "/lazy/" .. name, _ = { loaded = {} } }
                lazy_config.plugins[plugin.name] = plugin
                vim.opt.rtp:prepend(plugin.dir)
                if type(spec.config) == "function" then
                    spec.config(plugin, type(spec.opts) == "table" and spec.opts or {})
                elseif type(spec.opts) == "table" then
                    local mod = repo == "fixture/smoke.nvim" and "smoke"
                        or (repo == "lewis6991/gitsigns.nvim" and "gitsigns"
                        or (repo == "numToStr/Comment.nvim" and "Comment" or "gruber-darker"))
                    require(mod).setup(spec.opts)
                end
            end
        end
    end,
}
local ok, err = pcall(dofile, root .. "/src/nvim/tuim_init.lua")
assert(ok, err)
if scenario == "bootstrap_timeout" then
    assert(bootstrap_opts and bootstrap_opts.timeout <= 15000, "bootstrap clone must have a bounded timeout")
    assert(bootstrap_opts.env.GIT_TERMINAL_PROMPT == "0", "bootstrap must not wait for a credentials prompt")
    assert(not captured, "failed bootstrap must continue offline without initializing Lazy")
    assert(vim.g.tuim_plugins_disabled, "failed bootstrap must be exposed to the UI")
    print("PASS: timed-out bootstrap continues offline with bounded noninteractive clone")
    vim.cmd("qa!")
    return
elseif scenario == "safe_mode" then
    assert(not captured, "safe mode must not initialize lazy.nvim")
    assert(not _G.smoke_setup, "safe mode must not run plugin setup hooks")
    assert(vim.g.tuim_plugins_disabled, "safe mode must be exposed to the UI")
    print("PASS: offline plugin runtime safe mode bypasses plugin initialization")
    vim.cmd("qa!")
    return
end
assert(captured, "runtime must initialize lazy.nvim")
if scenario == "setup_error" or scenario == "require_error" then
    -- Run scheduled notices. A broken plugin must not prevent the editor starting.
    vim.wait(20, function() return false end)
    local reported = false
    for _, notice in ipairs(notices) do
        local expected = scenario == "setup_error" and "fixture setup failure" or "fixture module load failure"
        if notice[2]:find(expected, 1, true) then reported = true end
    end
    assert(reported, "plugin load/setup failure must be reported rather than silently marking initialized")
else
    assert(type(_G.smoke_setup) == "table", "bare user plugin must receive automatic setup()")
end
if scenario == "override" then
    assert(_G.smoke_setup.greeting == "configured", "per-plugin opts must reach setup()")
elseif scenario == "bad_config" then
    local reported = false
    for _, notice in ipairs(notices) do
        if notice[1] == "error" and notice[2]:find("syntax", 1, true) then reported = true end
    end
    assert(reported, "invalid user config must report a useful error and continue startup")
end
assert(type(_G.tuim_plugin_specs) == "table", "plugin specs must remain available for diagnostics")
local inventory_path = data .. "/plugin_inventory.json"
assert(vim.fn.filereadable(inventory_path) == 1, "plugin inventory must be persisted")
local inventory = vim.json.decode(table.concat(vim.fn.readfile(inventory_path), "\n"))
local found = false
local by_repo = {}
for _, item in ipairs(inventory) do
    by_repo[item.full_name] = item
    if item.full_name == "fixture/smoke.nvim" then found = true end
end
assert(found, "user-config plugin must appear in inventory")
if scenario == "recipe_options" then
    assert(type(_G.gruber_setup) == "table", "recipe plugin must receive setup()")
    assert(_G.gruber_setup.bold == false, "recipe option 'bold' must reach setup()")
    assert(type(_G.gruber_setup.italic) == "table" and _G.gruber_setup.italic.strings == false,
        "dotted recipe option 'italic.strings' must expand to nested table")
    assert(_G.gruber_setup.underline == true, "unspecified recipe default 'underline' must be filled")
    local item = assert(by_repo["blazkowolf/gruber-darker.nvim"])
    assert(item.supported == true and item.recipe_id == "gruber-darker", "supported recipe metadata missing")
    assert(item.lifecycle == "verified", "expected verified lifecycle, got: " .. tostring(item.lifecycle))
    assert(item.configured == true, "recipe_options must mark plugin configured")
    assert(item.pin_source == "recipe" and item.pinned_commit == "35cb97959ef01f7193c94c404c13ddb3d4346654",
        "recipe pin must be applied and recorded")
    assert(vim.fn.filereadable(data .. "/plugin_pending_restart.json") == 0,
        "pending restart marker must clear after clean startup")
elseif scenario == "recipe_override_precedence" then
    assert(type(_G.gruber_setup) == "table", "overridden recipe plugin must still run setup()")
    assert(_G.gruber_setup.bold == true and _G.gruber_setup.custom_flag == 42,
        "explicit user Lua opts override must win over recipe_options.json")
    local item = assert(by_repo["blazkowolf/gruber-darker.nvim"])
    assert(item.pin_source == "user" and item.pinned_commit == "abcdef1234567",
        "explicit user pin must override recipe pin")
    local spec_commit
    for _, s in ipairs(captured) do
        if s[1] == "blazkowolf/gruber-darker.nvim" then spec_commit = s.commit end
    end
    assert(spec_commit == "abcdef1234567", "lazy spec commit must match user pin")
    local comment_item = assert(by_repo["numToStr/Comment.nvim"])
    assert(comment_item.pin_source == "lua" and comment_item.pinned_commit == "v0.8.1-custom",
        "explicit Lua tag override must be reported as pin_source='lua' with exact ref, got: "
        .. tostring(comment_item.pin_source) .. "/" .. tostring(comment_item.pinned_commit))
elseif scenario == "recipe_verify_fail" then
    local item = assert(by_repo["lewis6991/gitsigns.nvim"])
    assert(item.lifecycle == "needs_attention",
        "missing verification export must mark lifecycle needs_attention, got: " .. tostring(item.lifecycle))
    assert(item.health_message:find("attach", 1, true),
        "health_message must identify missing export: " .. tostring(item.health_message))
    -- Re-calling _G.tuim_verify_plugin on a non-function export or failed setup must stay needs_attention.
    package.loaded["gitsigns"].attach = "not_a_function"
    local vres = _G.tuim_verify_plugin("lewis6991/gitsigns.nvim")
    assert(vres.lifecycle == "needs_attention", "non-function export must fail verification")
    assert(vres.health_message:find("not a function", 1, true),
        "non-function export must be reported explicitly: " .. tostring(vres.health_message))
    -- Disabled / uninitialized plugin must NOT trigger require() or become verified.
    local unloaded_repo = "numToStr/Comment.nvim"
    vim.fn.mkdir(data .. "/lazy/Comment.nvim/lua", "p")
    vim.fn.writefile({ 'error("must not autoload disabled/unloaded plugin during verify")' },
        data .. "/lazy/Comment.nvim/lua/Comment.lua")
    local v_unreg = _G.tuim_verify_plugin(unloaded_repo)
    assert(v_unreg.lifecycle == "not_installed", "unregistered plugin must stay not_installed")
elseif scenario == "unmatched_module_and_theme_sync" then
    assert(vim.g.colors_name == "gruber-darker",
        "saved installed theme must load at startup, got: " .. tostring(vim.g.colors_name))
    theme_events = {}
    _G.tuim_apply_theme("vscode")
    assert(vim.g.colors_name == "vscode", "vscode switch must activate")
    assert(#theme_events > 0 and theme_events[#theme_events].bg_editor == "#1f1f1f",
        "switching back to vscode with vscode.nvim loaded must still emit tuim_theme_changed")
    theme_events = {}
    _G.tuim_apply_theme("local-custom")
    assert(vim.g.colors_name == "local-custom", "local ~/.config/tuim/colors theme must load under --clean")
    assert(#theme_events > 0 and theme_events[#theme_events].bg_editor == "#123456",
        "local colorscheme must emit synced bg_editor")
    local miss_ok = pcall(_G.tuim_apply_theme, "nonexistent-theme")
    assert(not miss_ok, "missing theme must raise an error rather than failing silently")
end
print("PASS: offline plugin runtime (" .. scenario .. ") initializes user spec, setup hook and inventory")
vim.cmd("qa!")
