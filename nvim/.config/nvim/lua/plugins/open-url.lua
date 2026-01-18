return {
  "simonwinther/open-url.nvim",
  keys = {
    {
      "<leader>O",
      function()
        require("open_url").open_at_line()
      end,
      desc = "Open URL",
    },
  },
}
