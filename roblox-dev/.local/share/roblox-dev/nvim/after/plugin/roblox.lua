-- Supplemental Luau support loaded only by roblox-nvim.
-- The user's existing init.lua, plugin specifications and lockfile stay in place.
local resources = vim.fn.expand("~/.local/share/roblox-dev")
vim.filetype.add({
  extension = {
    luau = "luau",
    lua = function(_, buf)
      return vim.fs.root(buf, "default.project.json") and "luau" or "lua"
    end,
  },
})

local caps = vim.lsp.protocol.make_client_capabilities()
caps.workspace.didChangeWatchedFiles.dynamicRegistration = true
vim.lsp.config("roblox_luau", {
  cmd = {
    vim.fn.expand("~/.local/bin/luau-lsp"), "lsp",
    "--definitions:@roblox=" .. resources .. "/globalTypes.d.luau",
    "--docs=" .. resources .. "/api-docs.json",
  },
  capabilities = caps,
  filetypes = { "luau" },
  root_markers = { "default.project.json", "rokit.toml", ".git" },
  settings = { ["luau-lsp"] = { platform = { type = "roblox" }, sourcemap = { enabled = true, autogenerate = false } } },
  before_init = function(params, config)
    if not params.rootUri then return end
    local root = vim.uri_to_fname(params.rootUri)
    local project = root .. "/default.project.json"
    if vim.fn.filereadable(project) == 0 then return end
    local cache = (vim.env.XDG_CACHE_HOME or vim.fn.expand("~/.cache")) .. "/roblox-dev/sourcemaps"
    vim.fn.mkdir(cache, "p")
    local output = cache .. "/" .. vim.fn.sha256(project) .. ".json"
    local result = vim.system({ "rojo", "sourcemap", project, "--include-non-scripts", "--output", output }, { text = true }):wait(10000)
    if result.code == 0 then
      config.settings["luau-lsp"].sourcemap.sourcemapFile = output
    end
  end,
})
vim.lsp.enable("roblox_luau")

local lint_group = vim.api.nvim_create_augroup("RobloxDevLint", { clear = true })
vim.api.nvim_create_autocmd("FileType", {
  pattern = "luau",
  group = vim.api.nvim_create_augroup("RobloxDevSupport", { clear = true }),
  callback = function(args)
    pcall(vim.treesitter.start, args.buf, "luau")
    local ok, conform = pcall(require, "conform")
    if ok then
      conform.formatters_by_ft.luau = { "stylua" }
      conform.formatters.stylua = { command = vim.fn.expand("~/.local/bin/stylua") }
    end
    local has_lint, lint = pcall(require, "lint")
    if has_lint then
      lint.linters_by_ft.luau = { "selene" }
      -- The configuration affects this buffer's diagnostics only.
      local root = vim.fs.root(args.buf, "selene.toml")
      if root then
        vim.api.nvim_clear_autocmds({ group = lint_group, buffer = args.buf })
        vim.api.nvim_create_autocmd({ "BufWritePost", "InsertLeave" }, {
          group = lint_group,
          buffer = args.buf,
          callback = function() lint.try_lint("selene", { cwd = root }) end,
        })
      end
    end
  end,
})

-- roblox-nvim loads this after LazyVim has finished setting its runtime path.
local buffer = vim.api.nvim_get_current_buf()
if vim.bo.filetype == "lua" and vim.fs.root(buffer, "default.project.json") then
  vim.bo.filetype = "luau"
end
if vim.bo.filetype == "luau" then
  vim.api.nvim_exec_autocmds("FileType", { buffer = buffer })
end
