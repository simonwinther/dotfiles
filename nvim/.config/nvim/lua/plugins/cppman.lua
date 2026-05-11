return {
  dir = "~/dev/nvim/cppman.nvim",
  cmd = "CPPMan",
  keys = {
    {
      "<leader>cu",
      function()
        require("cppman").open_for(vim.fn.expand("<cword>"))
      end,
      desc = "[C++] open under cursor",
    },
    {
      "<leader>ck",
      function()
        require("cppman").search()
      end,
      desc = "[C++] keyword search",
    },
  },
  opts = {
    source = "both",
    viewer = {
      width = 0.94,
      height = 0.75,
    },
    picker = {
      width = 0.4,
      height = 0.3,
    },
  },
}
