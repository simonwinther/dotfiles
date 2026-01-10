return {
  "stevearc/oil.nvim",
  dependencies = {
    "nvim-tree/nvim-web-devicons",
    "mini.icons",
  },
  opts = {
    -- default_file_explorer = false,
    use_default_keymaps = true, -- keep Oil defaults unless we override them
    keymaps = {
      ["l"] = "actions.select", -- enter dir / open file
      ["h"] = "actions.parent", -- go up

      -- Inside Oil: "-" = open in *horizontal* split
      ["-"] = { "actions.select", opts = { horizontal = true } },

      -- Alt + | → vertical split (might be <A-Bslash> depending on layout)
      ["|"] = { "actions.select", opts = { vertical = true } },

      -- Disable Oil's own <C-s> so your save mapping can use it
      ["<C-s>"] = false,
      ["<C-l>"] = false, -- Refresh current directory list, consider changin to C-r
      ["<C-h>"] = false,
      ["<leader>q"] = "actions.send_to_qflist",
    },
    view_options = {
      show_hidden = true,
    },
  },
}
