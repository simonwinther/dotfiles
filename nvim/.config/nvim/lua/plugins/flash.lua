return {
  "folke/flash.nvim",
  event = "VeryLazy",
  lazy = true,
  opts = {
    modes = {
      search = {
        enabled = true,
      },
      char = {
        jump_labels = false, -- This is for f/F, t/T, I love it except, when using df' i need an extra <CR>
      },
    },
  },
  keys = {
    { "s", mode = { "n", "x", "o" }, "/", desc = "Flash" },
    { "S", mode = { "n", "x", "o" }, "?", desc = "Flash backward" },
    -- {
    --   "<c-<leader>>", -- DUPLICATE KEY
    --   mode = { "n", "x", "o" },
    --   function()
    --     require("flash").treesitter()
    --   end,
    --   desc = "Flash Treesitter",
    -- },
    {
      "<c-s>", -- DUPLICATE KEY: <c-s> is for saving
      mode = { "c" },
      function()
        require("flash").toggle()
      end,
      desc = "Toggle Flash Search",
    },
  },
}
