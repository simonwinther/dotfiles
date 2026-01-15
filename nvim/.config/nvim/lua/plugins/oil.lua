return {
  "stevearc/oil.nvim",

  cmd = "Oil",
  keys = {
    { "-", "<cmd>Oil<cr>", desc = "Open Parent Directory" },
  },

  dependencies = {
    "nvim-tree/nvim-web-devicons",
    "mini.icons",
  },

  opts = {
    use_default_keymaps = true,
    keymaps = {
      ["l"] = "actions.select",
      ["h"] = "actions.parent",

      -- These mappings only work INSIDE Oil
      ["-"] = { "actions.select", opts = { horizontal = true } },
      ["|"] = { "actions.select", opts = { vertical = true } },

      ["<C-s>"] = false,
      ["<C-l>"] = false,
      ["<C-h>"] = false,
      ["<leader>q"] = "actions.send_to_qflist",
    },
    view_options = {
      show_hidden = true,
    },
  },
}
