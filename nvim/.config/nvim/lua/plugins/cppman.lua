return {
  "simonwinther/cppman.nvim",
  version = "*",
  cmd = "CPPMan",
  keys = {
    {
      "<localleader>w",
      function()
        require("cppman").open_for(vim.fn.expand("<cword>"))
      end,
      desc = "[C++] open under cursor",
    },
    {
      "<localleader>s",
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
