-- Runs ONLY via scripts/test_plugin_recipes_live.py --online in an isolated HOME.
local base = assert(vim.env.GTUIM_RECIPE_TEST_DIR)
local recipes = vim.json.decode(table.concat(vim.fn.readfile(base .. "/recipes.json"), "\n"))
for _, recipe in ipairs(recipes) do vim.opt.rtp:prepend(recipe.test_dir) end
local function nested(root, key, value)
    local parts = vim.split(key, ".", { plain = true })
    for i = 1, #parts - 1 do
        root[parts[i]] = root[parts[i]] or {}
        root = root[parts[i]]
    end
    root[parts[#parts]] = value
end
for _, recipe in ipairs(recipes) do
    local opts = {}
    for _, field in ipairs(recipe.fields) do nested(opts, field.key, field.default) end
    local module = require(recipe.module)
    assert(type(module.setup) == "function", recipe.repo .. " missing setup()")
    module.setup(opts)
    local verification = recipe.verification
    if verification:sub(1, 7) == "module:" then
        local functions = verification:match("^module:[^:]+:(.*)$") or ""
        for name in functions:gmatch("[^,]+") do
            assert(type(module[name]) == "function", recipe.repo .. " missing API " .. name)
        end
    elseif verification:sub(1, 12) == "colorscheme:" then
        vim.cmd.colorscheme(verification:sub(13))
        assert(vim.g.colors_name == verification:sub(13), "colorscheme did not activate")
    end
    print("PASS: pinned " .. recipe.repo .. " setup with recipe defaults + API check")
end

-- A behavioral regression check: ignore='' would make Comment.nvim skip every line.
vim.cmd.enew()
vim.bo.filetype = "lua"
vim.bo.commentstring = "-- %s"
vim.api.nvim_buf_set_lines(0, 0, -1, false, { "local example = 1" })
require("Comment.api").toggle.linewise.current()
assert(vim.api.nvim_get_current_line() == "-- local example = 1", "Comment recipe must actually comment a line")
require("Comment.api").toggle.linewise.current()
assert(vim.api.nvim_get_current_line() == "local example = 1", "Comment recipe must uncomment a line")
print("PASS: Comment.nvim line toggle round trip")

-- Verify pairing rules are registered (without requiring real input/display events).
assert(require("nvim-autopairs").get_rule("(") ~= nil, "Autopairs default bracket rule missing")
print("PASS: nvim-autopairs bracket rule registered")

-- Verify gitsigns can attach to a real, isolated Git worktree and observe a hunk.
vim.cmd.edit(vim.fn.fnameescape(base .. "/fixture/example.lua"))
require("gitsigns").attach(0)
vim.api.nvim_buf_set_lines(0, 0, -1, false, { "local changed = 2" })
assert(vim.wait(5000, function()
    local status = vim.b.gitsigns_status_dict
    return status and (status.changed or 0) >= 1
end, 20), "Gitsigns failed to observe an edited line in fixture worktree")
print("PASS: gitsigns attached and observed changed line")
vim.cmd("qa!")
