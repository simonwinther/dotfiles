return {
  { "folke/tokyonight.nvim", enabled = false },
  {
    "catppuccin/nvim",
    name = "catppuccin",
    lazy = false,
    priority = 1000,
    build = ":CatppuccinCompile",
    opts = {
      flavour = "mocha",
      transparent_background = true, -- Set to true if you want your Ghostty terminal background to show through
      styles = {
        comments = { "italic" },
        keywords = { "bold" },
      },
    },
    config = function(_, opts)
      require("catppuccin").setup(opts)
      vim.cmd.colorscheme("catppuccin")
    end,
  },
}
