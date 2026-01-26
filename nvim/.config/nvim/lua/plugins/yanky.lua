-- FIX: Doesn't work perfect with my `gb` keymap
return {
  "gbprod/yanky.nvim",
  recommended = true,
  desc = "Better Yank/Paste",
  event = "LazyFile",
  opts = {
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
  },
}
