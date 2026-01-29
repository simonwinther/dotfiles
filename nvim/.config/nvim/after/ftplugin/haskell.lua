-- ~/.config/nvim/after/ftplugin/haskell.lua
local ht = require("haskell-tools")
local bufnr = vim.api.nvim_get_current_buf()
local opts = { noremap = true, silent = true, buffer = bufnr }

-- Hoogle search for the type signature of the definition under the cursor
vim.keymap.set(
  "n",
  "<localleader>t",
  ht.hoogle.hoogle_signature,
  vim.tbl_extend("force", opts, { desc = "Hoogle search for type signature" })
)

vim.keymap.set("n", "<localleader>s", function()
  vim.cmd("w")
  vim.cmd("botright split | resize 12 | terminal runghc %")
  local buf = vim.api.nvim_get_current_buf()
  vim.keymap.set("n", "q", "<cmd>close<cr>", { buffer = buf, silent = true })
end, { buffer = true, desc = "Run Haskell file" })
