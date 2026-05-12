return {
  {
    "nvim-treesitter/nvim-treesitter",
    branch = "main",
    build = ":TSUpdate",
  },
  {
    "MeanderingProgrammer/treesitter-modules.nvim",
    dependencies = { "nvim-treesitter/nvim-treesitter" },
    keys = {
      {
        "<A-o>",
        function()
          require("treesitter-modules").init_selection()
        end,
        mode = "n",
        desc = "Start Treesitter selection",
      },
      {
        "<A-o>",
        function()
          require("treesitter-modules").node_incremental()
        end,
        mode = "x",
        desc = "Expand Treesitter selection",
      },
      {
        "<A-i>",
        function()
          require("treesitter-modules").node_decremental()
        end,
        mode = "x",
        desc = "Shrink Treesitter selection",
      },
      {
        "<A-O>",
        function()
          require("treesitter-modules").scope_incremental()
        end,
        mode = "x",
        desc = "Expand Treesitter selection scope",
      },
    },
    opts = {
      incremental_selection = {
        enable = true,
        keymaps = {
          init_selection = "<A-o>",
          node_incremental = "<A-o>",
          scope_incremental = "<A-O>",
          node_decremental = "<A-i>",
        },
      },
    },
  },
}
