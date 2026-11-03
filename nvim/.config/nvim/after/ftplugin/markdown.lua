-- Markview needs nowrap to align and fully render tables.
vim.opt_local.wrap = false

vim.keymap.set("n", "\\mv", "<cmd>Markview toggle<cr>", {
  desc = "Toggle Markdown preview",
  buffer = true,
})

-- Paste Image (img-clip.nvim)
vim.keymap.set("n", "\\mp", "<cmd>PasteImage<cr>", {
  desc = "[img-clip.nvim] Paste image from system clipboard",
  buffer = true,
})
