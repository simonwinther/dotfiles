return {
  -- use vim-tmux-navigator
  {
    "christoomey/vim-tmux-navigator",
    cmd = {
      "TmuxNavigateLeft",
      "TmuxNavigateDown",
      "TmuxNavigateUp",
      "TmuxNavigateRight",
      "TmuxNavigatePrevious",
      "TmuxNavigatorProcessList",
    },
    init = function()
      vim.g.tmux_navigator_no_mappings = 1
    end,
    keys = {
      { "<C-h>", "<cmd>TmuxNavigateLeft<CR>", mode = "n", silent = true },
      { "<C-j>", "<cmd>TmuxNavigateDown<CR>", mode = "n", silent = true },
      { "<C-k>", "<cmd>TmuxNavigateUp<CR>", mode = "n", silent = true },
      { "<C-l>", "<cmd>TmuxNavigateRight<CR>", mode = "n", silent = true },
    },
  },
}
