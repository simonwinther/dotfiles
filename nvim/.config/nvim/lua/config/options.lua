-- Options are automatically loaded before lazy.nvim startup
-- Default options that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/options.lua
-- Add any additional options here

-- Disable clipboard sync with system
vim.opt.clipboard = ""

-- Keep pickers rooted at :pwd when opening files from nested projects.
vim.g.root_spec = { "cwd" }

-- Use basedpyright as the Python LSP instead of pyright
vim.g.lazyvim_python_lsp = "basedpyright"
