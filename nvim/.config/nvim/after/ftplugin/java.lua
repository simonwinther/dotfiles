-- Description: Keybindings and configurations for Java development in Neovim

-- Compile Java File for A1
vim.keymap.set("n", "\\r", function()
  require("snacks.terminal")("ant build && ant -Dlocaltest=false test;read", {
    win = { position = "float" },
    start_insert = false,
    on_open = function()
      vim.cmd("stopinsert")
    end,
  })
end, {
  buffer = true,
  desc = "Run Ant Tests",
})

-- Simple format-all command, bound to \F in normal mode
vim.keymap.set("n", "\\F", function()
  local files = vim.fn.systemlist('find . -type f -name "*.java"')

  for _, f in ipairs(files) do
    if f ~= "" then
      vim.cmd("edit " .. vim.fn.fnameescape(f))
      pcall(function()
        vim.lsp.buf.format({ async = false })
      end)
      vim.cmd("write")
    end
  end
end, { desc = "Format ALL Java files using jdtls" })
