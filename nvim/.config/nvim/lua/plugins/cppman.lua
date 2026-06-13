local dev_mode = false

return {
  {
    dir = "~/dev/nvim/cppman.nvim",
    name = "cppman.nvim-dev",
    cmd = "CPPManDev",
    enabled = dev_mode,
    init = function()
      vim.api.nvim_create_autocmd("FileType", {
        pattern = { "cpp", "c" },
        callback = function(args)
          vim.keymap.set("n", "<localleader>w", function()
            require("cppman").open_for(vim.fn.expand("<cword>"))
          end, { buffer = args.buf, desc = "DEV: [C++] open under cursor" })

          vim.keymap.set("n", "<localleader>s", function()
            require("cppman").search()
          end, { buffer = args.buf, desc = "DEV: [C++] keyword search" })
        end,
      })
    end,
  },

  {
    "simonwinther/cppman.nvim",
    name = "cppman.nvim",
    version = "*",
    cmd = "CPPMan",
    enabled = not dev_mode,
    init = function()
      vim.api.nvim_create_autocmd("FileType", {
        pattern = { "cpp", "c" },
        callback = function(args)
          vim.keymap.set("n", "<localleader>w", function()
            require("cppman").open_for(vim.fn.expand("<cword>"))
          end, { buffer = args.buf, desc = "[C++] open under cursor" })

          vim.keymap.set("n", "<localleader>s", function()
            require("cppman").search()
          end, { buffer = args.buf, desc = "[C++] keyword search" })
        end,
      })
    end,
  },
}
