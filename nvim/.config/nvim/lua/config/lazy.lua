local nix_root = vim.env.NVIM_NIX_PLUGIN_ROOT
local lazypath = nix_root and (nix_root .. "/lazy.nvim") or (vim.fn.stdpath("data") .. "/lazy/lazy.nvim")
if not (vim.uv or vim.loop).fs_stat(lazypath) then
  local lazyrepo = "https://github.com/folke/lazy.nvim.git"
  local out = vim.fn.system({ "git", "clone", "--filter=blob:none", "--branch=stable", lazyrepo, lazypath })
  if vim.v.shell_error ~= 0 then
    vim.api.nvim_echo({
      { "Failed to clone lazy.nvim:\n", "ErrorMsg" },
      { out, "WarningMsg" },
      { "\nPress any key to exit..." },
    }, true, {})
    vim.fn.getchar()
    os.exit(1)
  end
end
vim.opt.rtp:prepend(lazypath)

local pinned = {}
if nix_root then
  for name, kind in vim.fs.dir(nix_root) do
    if name ~= "runtime" then
      pinned[#pinned + 1] = { name = name, dir = assert(vim.uv.fs_realpath(nix_root .. "/" .. name)), build = false }
    end
  end
  pinned[#pinned + 1] = { "mason-org/mason.nvim", enabled = false }
  pinned[#pinned + 1] = { "mason-org/mason-lspconfig.nvim", enabled = false }
  pinned[#pinned + 1] = { "neovim/nvim-lspconfig", opts = function(_, opts)
    for name, server in pairs(opts.servers or {}) do
      if server == true then opts.servers[name] = { mason = false }
      elseif type(server) == "table" then server.mason = false end
    end
  end }
  pinned[#pinned + 1] = { "nvim-treesitter/nvim-treesitter", opts = function(_, opts)
    -- LazyVim extends parser lists, so a function must clear automatic installs.
    opts.ensure_installed = {}
    opts.auto_install = false
    opts.install_dir = nix_root .. "/runtime"
  end }
  pinned[#pinned + 1] = { "saghen/blink.cmp", opts = { fuzzy = { implementation = "rust", prebuilt_binaries = { download = false } } } }
end

require("lazy").setup({
  spec = {
    -- add LazyVim and import its plugins
    { "LazyVim/LazyVim", import = "lazyvim.plugins", opts = {
      colorscheme = "catppuccin",
    } },
    -- import/override with your plugins
    { import = "plugins" },
    pinned,
  },
  root = nix_root or (vim.fn.stdpath("data") .. "/lazy"),
  lockfile = nix_root and (vim.fn.stdpath("state") .. "/lazy-lock.json") or (vim.fn.stdpath("config") .. "/lazy-lock.json"),
  git = { enabled = not nix_root },
  rocks = { enabled = not nix_root },
  -- Where locally-developed plugins live (used by specs with `dev = true`).
  dev = { path = "~/dev/system/nvim" },
  defaults = {
    -- By default, only LazyVim plugins will be lazy-loaded. Your custom plugins will load during startup.
    -- If you know what you're doing, you can set this to `true` to have all your custom plugins lazy-loaded by default.
    lazy = false,
    -- It's recommended to leave version=false for now, since a lot the plugin that support versioning,
    -- have outdated releases, which may break your Neovim install.
    version = false, -- always use the latest git commit
    -- version = "*", -- try installing the latest stable version for plugins that support semver
  },
  install = { missing = not nix_root, colorscheme = { "catppuccin", "habamax" } },
  checker = {
    enabled = not nix_root, -- Nix owns plugin updates on NixOS.
    notify = false, -- notify on update
  }, -- automatically check for plugin updates
  performance = {
    rtp = {
      paths = nix_root and { nix_root .. "/runtime" } or {},
      -- disable some rtp plugins
      disabled_plugins = {
        "gzip",
        -- "matchit",
        -- "matchparen",
        "netrwPlugin",
        "tarPlugin",
        "tohtml",
        "tutor",
        "zipPlugin",
      },
    },
  },
})
