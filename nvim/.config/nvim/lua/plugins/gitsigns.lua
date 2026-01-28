return {
  "lewis6991/gitsigns.nvim",
  opts = {
    -- If I want the default which is a bit smaller, just delete `signs` and `signs_staged` tables
    signs = {
      add = { text = "▌" },
      change = { text = "▌" },
      delete = { text = "▌" },
      topdelete = { text = "▌" },
      changedelete = { text = "▌" },
      untracked = { text = "▌" },
    },
    signs_staged = {
      add = { text = "▌" },
      change = { text = "▌" },
      delete = { text = "▌" },
      topdelete = { text = "▌" },
      changedelete = { text = "▌" },
    },
    current_line_blame = true,
  },
}
