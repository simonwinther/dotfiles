return {
  "simonwinther/cppman.nvim",
  version = "*",
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
  dependencies = {
    "folke/snacks.nvim",
  },
}
