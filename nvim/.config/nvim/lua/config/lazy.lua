-- Home Manager supplies generation-specific paths here. Read them on every
-- editor start so an existing terminal cannot keep a stale plugin environment.
local nix_env = vim.fn.stdpath("config") .. "/nix-env.lua"
if vim.uv.fs_stat(nix_env) then
  dofile(nix_env)
end

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
  for name in vim.fs.dir(nix_root) do
    if name ~= "runtime" then
      -- Lazy identifies module owners by their directory name, so keep the
      -- named links instead of resolving them to anonymous Nix store sources.
      pinned[#pinned + 1] = { name = name, dir = nix_root .. "/" .. name, build = false }
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
    -- Home Manager links the packaged parsers and queries below this writable
    -- site directory. Nix owns their contents; Tree-sitter only discovers them.
    opts.install_dir = vim.fn.stdpath("data") .. "/site"
  end }
  pinned[#pinned + 1] = { "saghen/blink.cmp", opts = { fuzzy = {
    implementation = "rust",
    -- Nix pins the library and Lua sources together, without Git metadata.
    prebuilt_binaries = { download = false, ignore_version_mismatch = true },
  } } }
  pinned[#pinned + 1] = { "zbirenbaum/copilot.lua", opts = {
    server = { type = "binary", custom_server_filepath = vim.env.NVIM_COPILOT_SERVER or "copilot-language-server" },
  } }
  pinned[#pinned + 1] = { "simonwinther/cppman.nvim", opts = { index = { db_path = vim.env.NVIM_CPPMAN_INDEX } } }
end

require("lazy").setup({
  spec = {
    -- add LazyVim and import its plugins
    { "LazyVim/LazyVim", dir = nix_root and (nix_root .. "/LazyVim") or nil, import = "lazyvim.plugins", opts = {
      colorscheme = "catppuccin",
    } },
    -- import/override with your plugins
    { import = "plugins" },
    pinned,
  },
  -- Store plugins are local specs outside Lazy's managed installation root.
  root = vim.fn.stdpath("data") .. "/lazy",
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
