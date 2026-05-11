-- FIX: Doesn't work perfect with my `gb` keymap
return {
  "gbprod/yanky.nvim",
  recommended = true,
  desc = "Better Yank/Paste",
  event = "LazyFile",
  opts = {
    ring = {
      history_length = 50,
    },
    system_clipboard = {
      sync_with_ring = not vim.env.SSH_CONNECTION,
    },
    highlight = { timer = 150 },
    update_register_on_cycle = true,
    textobj = {
      enabled = true,
    },
  },
  keys = {
    {
      "<leader>sy",
      function()
        Snacks.picker.yanky()
      end,
      mode = { "n", "x" },
      desc = "[S]earch [Y]ank History",
    },
    { "p", "<Plug>(YankyPutAfter)", mode = { "n", "x" } },
    { "P", "<Plug>(YankyPutBefore)", mode = { "n", "x" } },
    { "gp", "<Plug>(YankyGPutAfter)", mode = { "n", "x" } },
    { "gP", "<Plug>(YankyGPutBefore)", mode = { "n", "x" } },

    { "<C-p>", "<Plug>(YankyPreviousEntry)", mode = "n" },
    { "<C-n>", "<Plug>(YankyNextEntry)", mode = "n" },

    -- tpope/vim-unimpaired style
    { "]p", "<Plug>(YankyPutIndentAfterLinewise)", mode = "n" },
    { "[p", "<Plug>(YankyPutIndentBeforeLinewise)", mode = "n" },
    { "]P", "<Plug>(YankyPutIndentAfterLinewise)", mode = "n" },
    { "[P", "<Plug>(YankyPutIndentBeforeLinewise)", mode = "n" },

    { ">p", "<Plug>(YankyPutIndentAfterShiftRight)", mode = "n" },
    { "<p", "<Plug>(YankyPutIndentAfterShiftLeft)", mode = "n" },
    { ">P", "<Plug>(YankyPutIndentBeforeShiftRight)", mode = "n" },
    { "<P", "<Plug>(YankyPutIndentBeforeShiftLeft)", mode = "n" },

    { "=p", "<Plug>(YankyPutAfterFilter)", mode = "n" },
    { "=P", "<Plug>(YankyPutBeforeFilter)", mode = "n" },
  },
}
