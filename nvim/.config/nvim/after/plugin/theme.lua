local function apply_highlights()
  local light = vim.o.background == "light"
  vim.api.nvim_set_hl(0, "FlashLabel", {
    bg = light and "#04a5e5" or "#89dceb",
    fg = light and "#eff1f5" or "#11111b",
    bold = true,
  })

  vim.api.nvim_set_hl(0, "CursorLineNr", { fg = light and "#1e66f5" or "#7aa2f7", bold = true })
  vim.api.nvim_set_hl(0, "LineNr", { fg = light and "#6c6f85" or "#a9b1d6", bold = false })
end

vim.api.nvim_create_autocmd("ColorScheme", {
  group = vim.api.nvim_create_augroup("UserThemeHighlights", { clear = true }),
  callback = apply_highlights,
})
apply_highlights()
