return {
  "simonwinther/open-url.nvim",
  dependencies = { "folke/snacks.nvim" },
  keys = {
    {
      "<leader>oul",
      function()
        require("open_url").open_at_line()
      end,
      desc = "Open URL (Line)",
    },
    {
      "<leader>oub",
      function()
        require("open_url").open_buffer()
      end,
      desc = "Open URL (Buffer)",
    },
  },
}
