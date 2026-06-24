local dev_mode = vim.env.CPPMAN_DEV == "1"

return {
  "simonwinther/cppman.nvim",
  dev = dev_mode,
  version = "*",
  cmd = "CPPMan",
  opts = {
    viewer = {
      border = "rounded",
      width = 0.95,
      height = 0.90,
    },
  },
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
}
