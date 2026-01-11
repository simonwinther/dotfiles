-- Paste Image (img-clip.nvim)
vim.keymap.set("n", "\\mp", "<cmd>PasteImage<cr>", {
  desc = "[img-clip.nvim] Paste image from system clipboard",
  buffer = true,
})
