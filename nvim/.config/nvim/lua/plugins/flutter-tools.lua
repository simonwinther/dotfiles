return {
  "nvim-flutter/flutter-tools.nvim",
  ft = { "dart" },
  -- Selection prompts use the vim.ui.select provider from snacks.nvim.
  dependencies = { "nvim-lua/plenary.nvim" },
  config = function()
    require("flutter-tools").setup({})
  end,
}
