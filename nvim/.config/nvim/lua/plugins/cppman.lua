local dev_mode = true

return {
  {
    dir = "~/dev/nvim/cppman.nvim",
    name = "cppman.nvim-dev",
    cmd = "CPPManDev",
    enabled = dev_mode,
    keys = {
      {
        "<localleader>w",
        function()
          require("cppman").open_for(vim.fn.expand("<cword>"))
        end,
        desc = "DEV: [C++] open under cursor",
      },
      {
        "<localleader>s",
        function()
          require("cppman").search()
        end,
        desc = "DEV: [C++] keyword search",
      },
    },
  },

  {
    "simonwinther/cppman.nvim",
    name = "cppman.nvim",
    version = "*",
    cmd = "CPPMan",
    enabled = not dev_mode,
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
  },
}
