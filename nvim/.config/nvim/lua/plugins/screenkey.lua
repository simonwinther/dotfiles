return {
  "NStefan002/screenkey.nvim",
  version = "*",
  lazy = true,
  opts = {
    win_opts = {
      height = 1,
      title = "Keys",
    },
  },
  keys = {
    {
      "<leader>ut",
      function()
        require("screenkey").toggle()
      end,
      desc = "Toggle key presses (screenkey)",
    },
  },
}
