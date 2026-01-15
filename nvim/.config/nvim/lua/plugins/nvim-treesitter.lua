return {
  "nvim-treesitter/nvim-treesitter",
  opts = function(_, opts)
    local ensure_installed = opts.ensure_installed
    vim.list_extend(ensure_installed, { "latex", "bibtex" })
  end,
}
